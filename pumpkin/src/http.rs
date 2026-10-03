//! The plugin's `wasi:http` GET, shared by the update checker and the cloud
//! thesaurus.
//!
//! Both call sites need the same thing: one HTTPS `GET`, the whole body as
//! UTF-8, no redirects, a hard timeout. The WASI client is only linked into the
//! `wasm32-wasip2` build, so the native stub below keeps `cargo test` and
//! `cargo check` offline.

/// Bytes read per `blocking-read` call while draining the response body.
pub const READ_CHUNK_BYTES: u64 = 8192;

/// Performs a `GET` and returns the body decoded as UTF-8.
///
/// `headers` are set in order; a non-`2xx` status is an error, which is how both
/// callers treat it (`UpdateChecker`'s status check and
/// `DefaultFilterManager`'s failed `openConnection()`).
#[cfg(target_arch = "wasm32")]
pub fn get(url: &str, headers: &[(&str, &str)], timeout_seconds: u64) -> Result<String, String> {
    use wasip2::http::outgoing_handler;
    use wasip2::http::types::{
        Headers, IncomingBody, Method, OutgoingBody, OutgoingRequest, RequestOptions, Scheme,
    };
    use wasip2::io::poll::poll;
    use wasip2::io::streams::StreamError;

    let (https, authority, path) = split_url(url)?;

    let headers_resource = Headers::new();
    for (name, value) in headers {
        headers_resource
            .set(name, &[value.as_bytes().to_vec()])
            .map_err(|error| format!("could not set the {name} header: {error:?}"))?;
    }

    let request = OutgoingRequest::new(headers_resource);
    request
        .set_method(&Method::Get)
        .map_err(|()| "could not set the request method".to_string())?;
    request
        .set_scheme(Some(&if https {
            Scheme::Https
        } else {
            Scheme::Http
        }))
        .map_err(|()| "could not set the request scheme".to_string())?;
    request
        .set_authority(Some(&authority))
        .map_err(|()| "could not set the request authority".to_string())?;
    request
        .set_path_with_query(Some(&path))
        .map_err(|()| "could not set the request path".to_string())?;

    // A `GET` still has to close its (empty) body before it is sent.
    {
        let body = request
            .body()
            .map_err(|()| "could not open the request body".to_string())?;
        let stream = body
            .write()
            .map_err(|()| "could not open the request body stream".to_string())?;
        drop(stream);
        OutgoingBody::finish(body, None)
            .map_err(|error| format!("could not finish the request body: {error:?}"))?;
    }

    let timeout = timeout_seconds * 1_000_000_000;
    let options = RequestOptions::new();
    options
        .set_connect_timeout(Some(timeout))
        .map_err(|()| "could not set the connect timeout".to_string())?;
    options
        .set_first_byte_timeout(Some(timeout))
        .map_err(|()| "could not set the first-byte timeout".to_string())?;
    options
        .set_between_bytes_timeout(Some(timeout))
        .map_err(|()| "could not set the between-bytes timeout".to_string())?;

    let future = outgoing_handler::handle(request, Some(options))
        .map_err(|error| format!("{url} rejected: {error:?}"))?;
    let response = loop {
        match future.get() {
            Some(Ok(Ok(response))) => break response,
            Some(Ok(Err(error))) => return Err(format!("{url} failed: {error:?}")),
            Some(Err(())) => return Err(format!("the {url} request was already consumed")),
            None => {
                let pollable = future.subscribe();
                let _ = poll(&[&pollable]);
            }
        }
    };

    let status = response.status();
    if !(200..300).contains(&status) {
        return Err(format!("{url} returned HTTP {status}"));
    }

    let incoming = response
        .consume()
        .map_err(|()| format!("could not consume the {url} response"))?;
    let stream = incoming
        .stream()
        .map_err(|()| format!("could not open the {url} response stream"))?;
    let mut bytes = Vec::new();
    loop {
        match stream.blocking_read(READ_CHUNK_BYTES) {
            Ok(chunk) if chunk.is_empty() => break,
            Ok(chunk) => bytes.extend_from_slice(&chunk),
            Err(StreamError::Closed) => break,
            Err(error) => return Err(format!("could not read the {url} response: {error:?}")),
        }
    }
    drop(stream);
    drop(IncomingBody::finish(incoming));

    String::from_utf8(bytes).map_err(|error| format!("the {url} response is not UTF-8: {error}"))
}

/// Native builds have no WASI HTTP client; both callers report a failed request,
/// so unit tests and `cargo check` stay offline.
#[cfg(not(target_arch = "wasm32"))]
pub fn get(_url: &str, _headers: &[(&str, &str)], _timeout_seconds: u64) -> Result<String, String> {
    Err("the WASI HTTP client is only available in the wasm32-wasip2 build".to_string())
}

/// Splits `scheme://authority/path?query` into the three parts the WIT request
/// takes. Only `http`/`https` are accepted: the plugin declares exactly those
/// network permissions.
pub fn split_url(url: &str) -> Result<(bool, String, String), String> {
    let (https, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (false, rest)
    } else {
        return Err(format!("unsupported URL scheme: {url}"));
    };
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return Err(format!("the URL has no host: {url}"));
    }
    Ok((https, authority.to_string(), path.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_splitting_follows_the_wit_request() {
        assert_eq!(
            split_url("https://api.github.com/repos/a/b/releases/latest").unwrap(),
            (
                true,
                "api.github.com".to_string(),
                "/repos/a/b/releases/latest".to_string()
            )
        );
        assert_eq!(
            split_url("http://example.invalid:8080/db.json?v=1").unwrap(),
            (
                false,
                "example.invalid:8080".to_string(),
                "/db.json?v=1".to_string()
            )
        );
        // A bare authority keeps the WIT's mandatory path.
        assert_eq!(
            split_url("https://example.invalid").unwrap(),
            (true, "example.invalid".to_string(), "/".to_string())
        );
        assert!(split_url("ftp://example.invalid/x").is_err());
        assert!(split_url("https:///x").is_err());
    }
}
