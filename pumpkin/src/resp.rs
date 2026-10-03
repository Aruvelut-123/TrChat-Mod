//! A minimal RESP2 client — the Rust twin of the Mod's `RespConnection`
//! (`redis/RespConnection.java`).
//!
//! The Mod can park a daemon thread on a blocking `SUBSCRIBE`
//! (`RedisBridge.subscriptionLoop`); this plugin is a single-threaded
//! `wasm32-wasip2` component with no thread to spare, so its subscriber is
//! *polled* from a scheduled task instead. That needs a read which can stop
//! halfway through a reply and pick up on the next tick:
//! [`RespConnection::poll`] buffers whatever has arrived and only consumes a
//! value once every byte of it is present, reporting `Ok(None)` while the
//! socket has nothing more to give.
//!
//! Everything else mirrors the Mod's client command for command: the same
//! `*N` array-of-bulk-strings requests, the same `+`/`-`/`:`/`$`/`*` reply
//! markers, and the same `Redis closed the connection` / `Redis error: …` /
//! `Unsupported Redis RESP marker: …` failures.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// One decoded RESP2 value — what `RespConnection.read` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// `+OK`
    Status(String),
    /// `:1`
    Integer(i64),
    /// `$5\r\nhello`; `None` for the `$-1` null bulk string.
    Bulk(Option<String>),
    /// `*2\r\n…`; `None` for the `*-1` null array.
    Array(Option<Vec<Value>>),
}

impl Value {
    /// The value as text — the shape of every reply the bridge inspects apart
    /// from the `PUBLISH` counter (`OK` acknowledgements, `message` pushes).
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Status(text) | Value::Bulk(Some(text)) => Some(text),
            _ => None,
        }
    }

    /// The value as an integer — the `PUBLISH` subscriber count.
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Value::Integer(number) => Some(*number),
            _ => None,
        }
    }

    /// The value as an array — a `SUBSCRIBE` acknowledgement or a push.
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(Some(values)) => Some(values),
            _ => None,
        }
    }
}

impl fmt::Display for Value {
    /// Renders the value the way the Mod's error messages interpolate the raw
    /// reply object.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Status(text) | Value::Bulk(Some(text)) => write!(formatter, "{text}"),
            Value::Bulk(None) | Value::Array(None) => formatter.write_str("null"),
            Value::Integer(number) => write!(formatter, "{number}"),
            Value::Array(Some(values)) => {
                formatter.write_str("[")?;
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(", ")?;
                    }
                    write!(formatter, "{value}")?;
                }
                formatter.write_str("]")
            }
        }
    }
}

/// The deepest reply nesting accepted while parsing.
///
/// The replies this bridge sends and receives are flat (`SUBSCRIBE`,
/// `PUBLISH`, `message`); the bound only keeps a malformed or hostile stream
/// from recursing the guest's stack away.
const MAX_DEPTH: usize = 32;

/// A TCP connection speaking RESP2.
pub struct RespConnection {
    stream: TcpStream,
    /// Bytes received but not yet consumed by a complete reply — a poll may end
    /// in the middle of one (`set_nonblocking`, or a receive timeout).
    pending: Vec<u8>,
}

impl RespConnection {
    /// Connects to `host:port`.
    ///
    /// Mirrors `Socket.connect(new InetSocketAddress(host, port), timeout)`
    /// (`RespConnection.java:23-24`): the host is resolved first, then each
    /// address gets the full connect timeout. `setKeepAlive(true)` has no
    /// `std` counterpart on this target; the Nagle switch does
    /// (`RespConnection.java:25-26`).
    pub fn connect(host: &str, port: u16, connect_timeout: Duration) -> io::Result<Self> {
        let addresses: Vec<_> = (host, port).to_socket_addrs()?.collect();
        if addresses.is_empty() {
            return Err(io::Error::other(format!(
                "Redis host '{host}' did not resolve"
            )));
        }
        let mut last_error = None;
        for address in addresses {
            match TcpStream::connect_timeout(&address, connect_timeout) {
                Ok(stream) => {
                    stream.set_nodelay(true)?;
                    return Ok(Self {
                        stream,
                        pending: Vec::new(),
                    });
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| io::Error::other("Redis connection failed")))
    }

    /// `Socket.setSoTimeout` (`RespConnection.java:27`) — the receive timeout a
    /// blocking read honours.
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.stream.set_read_timeout(timeout)
    }

    /// The send-side timeout. The Mod leaves this at the OS default; the port
    /// sets one so a stalled Redis cannot hold a scheduled task forever.
    pub fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.stream.set_write_timeout(timeout)
    }

    /// Switches the socket to non-blocking reads, so a poll never waits.
    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        self.stream.set_nonblocking(nonblocking)
    }

    /// `RespConnection.write` — the request as a RESP array of bulk strings.
    ///
    /// Lengths are byte counts, matching `part.getBytes(UTF_8).length`.
    pub fn write_command(&mut self, parts: &[&str]) -> io::Result<()> {
        let mut payload = Vec::with_capacity(24);
        payload.extend_from_slice(format!("*{}\r\n", parts.len()).as_bytes());
        for part in parts {
            payload.extend_from_slice(format!("${}\r\n", part.len()).as_bytes());
            payload.extend_from_slice(part.as_bytes());
            payload.extend_from_slice(b"\r\n");
        }
        self.stream.write_all(&payload)?;
        self.stream.flush()
    }

    /// `RespConnection.command` — write, then read exactly one reply.
    ///
    /// A reply that never arrives is an error, matching the
    /// `SocketTimeoutException` the Mod's blocking socket raises.
    pub fn command(&mut self, parts: &[&str]) -> io::Result<Value> {
        self.write_command(parts)?;
        self.poll()?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "Redis did not answer in time",
            )
        })
    }

    /// Consumes the next complete reply, if one has arrived.
    ///
    /// `Ok(None)` means "nothing complete yet": the socket had no more bytes
    /// (non-blocking) or the receive timeout expired. A partially received
    /// reply stays buffered for the next call, so a slow link cannot split a
    /// message in half.
    pub fn poll(&mut self) -> io::Result<Option<Value>> {
        loop {
            if let Some((value, consumed)) = parse(&self.pending, 0)? {
                self.pending.drain(..consumed);
                return Ok(Some(value));
            }
            let mut chunk = [0u8; 8192];
            match self.stream.read(&mut chunk) {
                // `RespConnection.read`: a closed socket is an EOF error.
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Redis closed the connection",
                    ))
                }
                Ok(read) => self.pending.extend_from_slice(&chunk[..read]),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error)
                    if error.kind() == io::ErrorKind::WouldBlock
                        || error.kind() == io::ErrorKind::TimedOut =>
                {
                    return Ok(None)
                }
                Err(error) => return Err(error),
            }
        }
    }
}

/// `RespConnection.requireOkay` — the `+OK` an `AUTH` or `SELECT` must answer
/// with before the connection is usable.
pub fn require_ok(response: &Value, command: &str) -> io::Result<()> {
    match response.as_str() {
        Some(text) if text.eq_ignore_ascii_case("OK") => Ok(()),
        _ => Err(io::Error::other(format!(
            "Unexpected Redis {command} response: {response}"
        ))),
    }
}

/// Parses one RESP2 value off the front of `bytes`, returning it with the
/// number of bytes it occupied — or `None` when `bytes` does not hold all of
/// it yet.
fn parse(bytes: &[u8], depth: usize) -> io::Result<Option<(Value, usize)>> {
    if depth > MAX_DEPTH {
        return Err(io::Error::other("Redis reply nested too deeply"));
    }
    let Some(&marker) = bytes.first() else {
        return Ok(None);
    };
    match marker {
        b'+' => Ok(line(bytes)?.map(|(text, used)| (Value::Status(text), used))),
        b'-' => match line(bytes)? {
            Some((text, _)) => Err(io::Error::other(format!("Redis error: {text}"))),
            None => Ok(None),
        },
        b':' => match line(bytes)? {
            Some((text, used)) => {
                let number = text.parse::<i64>().map_err(|_| {
                    io::Error::other(format!("Unsupported Redis RESP marker: :{text}"))
                })?;
                Ok(Some((Value::Integer(number), used)))
            }
            None => Ok(None),
        },
        b'$' => {
            let Some((text, header)) = line(bytes)? else {
                return Ok(None);
            };
            let length = text
                .parse::<i64>()
                .map_err(|_| io::Error::other(format!("Unsupported Redis RESP marker: ${text}")))?;
            if length < 0 {
                return Ok(Some((Value::Bulk(None), header)));
            }
            let end = header + length as usize;
            if bytes.len() < end + 2 {
                return Ok(None);
            }
            if &bytes[end..end + 2] != b"\r\n" {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Incomplete Redis bulk string",
                ));
            }
            let value = String::from_utf8_lossy(&bytes[header..end]).into_owned();
            Ok(Some((Value::Bulk(Some(value)), end + 2)))
        }
        b'*' => {
            let Some((text, header)) = line(bytes)? else {
                return Ok(None);
            };
            let length = text
                .parse::<i64>()
                .map_err(|_| io::Error::other(format!("Unsupported Redis RESP marker: *{text}")))?;
            if length < 0 {
                return Ok(Some((Value::Array(None), header)));
            }
            let mut values = Vec::with_capacity(length.min(1024) as usize);
            let mut cursor = header;
            for _ in 0..length {
                match parse(&bytes[cursor..], depth + 1)? {
                    Some((value, used)) => {
                        values.push(value);
                        cursor += used;
                    }
                    None => return Ok(None),
                }
            }
            Ok(Some((Value::Array(Some(values)), cursor)))
        }
        other => Err(io::Error::other(format!(
            "Unsupported Redis RESP marker: {}",
            other as char
        ))),
    }
}

/// Reads one `\r\n`-terminated line after a marker byte (`readLine`).
fn line(bytes: &[u8]) -> io::Result<Option<(String, usize)>> {
    let Some(position) = find_crlf(bytes, 1) else {
        return Ok(None);
    };
    let text = String::from_utf8_lossy(&bytes[1..position]).into_owned();
    Ok(Some((text, position + 2)))
}

/// The index of the next `\r\n` at or after `from`.
fn find_crlf(bytes: &[u8], from: usize) -> Option<usize> {
    let tail = bytes.get(from..)?;
    tail.windows(2)
        .position(|pair| pair == b"\r\n")
        .map(|offset| from + offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(input: &[u8]) -> Option<(Value, usize)> {
        parse(input, 0).expect("a decodable reply")
    }

    #[test]
    fn parses_the_five_markers() {
        assert_eq!(
            parsed(b"+OK\r\n"),
            Some((Value::Status("OK".to_string()), 5))
        );
        assert_eq!(parsed(b":42\r\n"), Some((Value::Integer(42), 5)));
        assert_eq!(
            parsed(b"$5\r\nhello\r\n"),
            Some((Value::Bulk(Some("hello".to_string())), 11))
        );
        assert_eq!(parsed(b"$-1\r\n"), Some((Value::Bulk(None), 5)));
        assert_eq!(parsed(b"*-1\r\n"), Some((Value::Array(None), 5)));
        assert_eq!(
            parsed(b"*2\r\n$7\r\nmessage\r\n$5\r\nhello\r\n"),
            Some((
                Value::Array(Some(vec![
                    Value::Bulk(Some("message".to_string())),
                    Value::Bulk(Some("hello".to_string())),
                ])),
                28
            ))
        );
    }

    #[test]
    fn empty_array_and_empty_bulk() {
        assert_eq!(
            parsed(b"*0\r\n"),
            Some((Value::Array(Some(Vec::new())), 4))
        );
        assert_eq!(
            parsed(b"$0\r\n\r\n"),
            Some((Value::Bulk(Some(String::new())), 6))
        );
    }

    #[test]
    fn incomplete_replies_wait_for_the_rest() {
        // Nothing at all.
        assert_eq!(parse(b"", 0).unwrap(), None);
        // A bulk string whose body has not fully arrived.
        assert_eq!(parse(b"$5\r\nhel", 0).unwrap(), None);
        // A header split across reads.
        assert_eq!(parse(b"$1", 0).unwrap(), None);
        assert_eq!(parse(b"*2\r\n$7\r\nmessage\r\n$5\r\nhel", 0).unwrap(), None);
    }

    #[test]
    fn error_replies_become_io_errors() {
        let error = parse(b"-WRONGTYPE nope\r\n", 0).unwrap_err();
        assert_eq!(error.to_string(), "Redis error: WRONGTYPE nope");
    }

    #[test]
    fn unsupported_markers_are_rejected() {
        let error = parse(b"%2\r\n", 0).unwrap_err();
        assert_eq!(error.to_string(), "Unsupported Redis RESP marker: %");
    }

    #[test]
    fn a_truncated_bulk_terminator_is_rejected() {
        let error = parse(b"$5\r\nhelloXX", 0).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn a_bulk_string_keeps_utf8() {
        // "喵" is three UTF-8 bytes.
        assert_eq!(
            parsed("$3\r\n喵\r\n".as_bytes()),
            Some((Value::Bulk(Some("喵".to_string())), 9))
        );
    }

    #[test]
    fn deep_nesting_is_refused() {
        let mut input = Vec::new();
        for _ in 0..(MAX_DEPTH + 2) {
            input.extend_from_slice(b"*1\r\n");
        }
        input.extend_from_slice(b"$0\r\n\r\n");
        assert!(parse(&input, 0).is_err());
    }

    #[test]
    fn display_matches_the_mod_error_text() {
        assert_eq!(Value::Integer(3).to_string(), "3");
        assert_eq!(Value::Bulk(None).to_string(), "null");
        assert_eq!(
            Value::Array(Some(vec![Value::Status("OK".to_string()), Value::Integer(1)]))
                .to_string(),
            "[OK, 1]"
        );
    }

    #[test]
    fn require_ok_is_case_insensitive() {
        assert!(require_ok(&Value::Status("ok".to_string()), "AUTH").is_ok());
        let error = require_ok(&Value::Status("NOAUTH".to_string()), "AUTH").unwrap_err();
        assert_eq!(
            error.to_string(),
            "Unexpected Redis AUTH response: NOAUTH"
        );
    }
}
