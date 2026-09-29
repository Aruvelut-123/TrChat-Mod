//! Chat functions — the `function.yml` `Mention` / `Mention-All` renderer,
//! ported from the Bukkit v2 `ChatFunctionService.process`.
//!
//! Behaviour follows `docs/spec/placeholder-function-filter.md`:
//!
//! * token priorities — Mention-All **600** > Mention **500** (§2.5),
//! * `process` — legacy codes stripped, start-asc / priority-desc sort,
//!   greedy non-overlapping accept, per-token `canUse` (§2.6),
//! * `canUse` — permission node then cooldown; OP level ≥ 2 bypasses the
//!   cooldown and never records one; the same function is granted only once
//!   per message (§2.7),
//! * rendering — Mention renders `"@" + 真实名` in AQUA with a
//!   `Function-Mention-Hover` hover; Mention-All renders the hardcoded
//!   `"@所有人"` (never localized) in GOLD + BOLD with a
//!   `Function-Mention-All-Hover` hover (§2.9),
//! * notify — `Notify == true` functions add their targets to the mentioned
//!   list; Mention-All adds every online player except the sender.
//!
//! The mention pattern matcher is deliberately regex-free: the plugin crate
//! has no regex dependency, so only the default `@? ?(names)` shape (optional
//! `@`, one optional space, literal prefix, exactly one `(names)` alternation
//! and nothing after it) is supported. Any other configured `Pattern` is
//! skipped with a warning — the same family of documented simplification as
//! the format-parser follow-up.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use pumpkin_plugin_api::{
    common::NamedColor,
    permission::PermissionLevel,
    player::Player,
    text::TextComponent,
    Server,
};

use crate::config::TrChatConfig;
use crate::lang;

/// Token priorities from the spec table (§2.5). Only the two implemented
/// built-ins are listed; custom functions would live below these.
const PRIORITY_MENTION_ALL: i64 = 600;
const PRIORITY_MENTION: i64 = 500;

/// Name of the built-in `General` section for player mentions.
pub const NAME_MENTION: &str = "Mention";
/// Name of the built-in `General` section for everyone-mentions.
pub const NAME_MENTION_ALL: &str = "Mention-All";

/// Outcome of running the function pipeline over one message body.
pub struct FunctionOutcome {
    /// The processed body: legacy codes are stripped and matched functions
    /// are replaced by their display text (`@Steve`, `@所有人`, …). Byte
    /// offsets in [`FunctionOutcome::spans`] refer to this string.
    pub body: String,
    /// Highlight spans into `body` (byte ranges), in message order.
    pub spans: Vec<Span>,
    /// Lowercased names of players to notify when they really receive the
    /// broadcast (`Notify` functions only).
    pub mentioned: HashSet<String>,
}

/// One highlighted span inside the processed body.
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: SpanKind,
}

/// What a span renders to (§2.9).
pub enum SpanKind {
    /// `Mention` — `@Name` in AQUA with the `Function-Mention-Hover` text.
    Mention { target: String },
    /// `Mention-All` — the hardcoded `@所有人` in GOLD + BOLD.
    MentionAll,
}

/// A mention token collected from the body before sorting/overlap resolution.
struct Token {
    start: usize,
    end: usize,
    priority: i64,
    function: String,
    kind: TokenKind,
    gate: Gate,
}

enum TokenKind {
    Mention { target: String },
    MentionAll,
}

/// §2.7 — the permission node and cooldown of the owning function.
#[derive(Clone)]
struct Gate {
    permission: String,
    cooldown_millis: i64,
}

/// Per-function cooldowns keyed by `lowercased-name:function` — the WIT
/// `uuid` type has no string form yet, so names key the map (same convention
/// as `chat::PlayerChatState`).
static FUNCTION_COOLDOWNS: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();

fn cooldowns() -> &'static Mutex<HashMap<String, Instant>> {
    FUNCTION_COOLDOWNS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Runs `function.yml` over the routed message body.
///
/// Returns `None` when nothing matched (the caller keeps the plain legacy
/// template render); `Some` carries the processed body, highlight spans and
/// the notify target set.
pub fn process(
    server: &Server,
    sender: &Player,
    body: &str,
    config: &TrChatConfig,
    disabled: &[String],
) -> Option<FunctionOutcome> {
    let fns = &config.function;
    let mention = fns
        .general
        .iter()
        .find(|f| f.name == NAME_MENTION && f.enabled && !disabled.contains(&f.name));
    let mention_all = fns
        .general
        .iter()
        .find(|f| f.name == NAME_MENTION_ALL && f.enabled && !disabled.contains(&f.name));
    if mention.is_none() && mention_all.is_none() {
        return None;
    }

    // §2.6.1 — the Mod strips legacy codes *before* scanning, so `&c@Steve`
    // cannot hide a mention behind a colour code (and players cannot colour
    // the body: §7.9).
    let stripped = strip_legacy_codes(body);
    let body: &str = &stripped;

    let online = server.get_all_players();
    let sender_name = sender.get_name();

    // ---- collect tokens (§2.5) ----
    let mut tokens: Vec<Token> = Vec::new();
    if let Some(f) = mention {
        let gate = Gate {
            permission: f.permission.clone(),
            cooldown_millis: f.cooldown_millis,
        };
        // Online names (sender excluded unless `Self-Mention`), longest first
        // so a longer name wins over its substrings.
        let mut names: Vec<String> = online
            .iter()
            .map(|p| p.get_name())
            .filter(|n| f.self_mention || !n.eq_ignore_ascii_case(&sender_name))
            .collect();
        names.sort_by(|a, b| b.len().cmp(&a.len()));
        if !names.is_empty() {
            for (start, end, target) in scan_mention(body, &names, &f.pattern) {
                tokens.push(Token {
                    start,
                    end,
                    priority: PRIORITY_MENTION,
                    function: f.name.clone(),
                    kind: TokenKind::Mention { target },
                    gate: gate.clone(),
                });
            }
        }
    }
    if let Some(f) = mention_all {
        let gate = Gate {
            permission: f.permission.clone(),
            cooldown_millis: f.cooldown_millis,
        };
        for key in &f.keys {
            for (start, end) in scan_literal_ci(body, key) {
                tokens.push(Token {
                    start,
                    end,
                    priority: PRIORITY_MENTION_ALL,
                    function: f.name.clone(),
                    kind: TokenKind::MentionAll,
                    gate: gate.clone(),
                });
            }
        }
    }
    if tokens.is_empty() {
        return None;
    }

    // ---- accept & overlap resolution (§2.6: start asc, priority desc) ----
    tokens.sort_by(|a, b| a.start.cmp(&b.start).then(b.priority.cmp(&a.priority)));
    let mut accepted: Vec<Token> = Vec::new();
    let mut cursor = 0usize;
    for t in tokens {
        if t.start >= cursor {
            cursor = t.end;
            accepted.push(t);
        }
    }
    if accepted.is_empty() {
        return None;
    }

    // ---- render (§2.6.5–2.9) ----
    let mut out_body = String::new();
    let mut spans = Vec::new();
    let mut mentioned = HashSet::new();
    let mut cooldown_granted = HashSet::new();
    let mut cursor = 0usize;
    for t in &accepted {
        out_body.push_str(&body[cursor..t.start]);
        let can_use =
            can_use_function(sender, &t.function, &t.gate, &mut cooldown_granted);
        if !can_use {
            // §2.6.6 — rejected tokens render as their original text.
            out_body.push_str(&body[t.start..t.end]);
            cursor = t.end;
            continue;
        }
        match &t.kind {
            TokenKind::Mention { target } => {
                // `@?` is optional in the source; the output always carries `@`.
                let start = out_body.len();
                out_body.push('@');
                out_body.push_str(target);
                spans.push(Span {
                    start,
                    end: out_body.len(),
                    kind: SpanKind::Mention {
                        target: target.clone(),
                    },
                });
                if let Some(f) = mention {
                    if f.notify && !target.eq_ignore_ascii_case(&sender_name) {
                        mentioned.insert(target.to_ascii_lowercase());
                    }
                }
            }
            TokenKind::MentionAll => {
                let start = out_body.len();
                out_body.push_str("@所有人");
                spans.push(Span {
                    start,
                    end: out_body.len(),
                    kind: SpanKind::MentionAll,
                });
                if let Some(f) = mention_all {
                    if f.notify {
                        for p in &online {
                            let n = p.get_name();
                            if !n.eq_ignore_ascii_case(&sender_name) {
                                mentioned.insert(n.to_ascii_lowercase());
                            }
                        }
                    }
                }
            }
        }
        cursor = t.end;
    }
    out_body.push_str(&body[cursor..]);

    Some(FunctionOutcome {
        body: out_body,
        spans,
        mentioned,
    })
}

/// §2.6.1 — `LegacyText.stripLegacyCodes`: removes every `&`/`§` colour or
/// format code (the symbol plus the following code character). A trailing
/// lone symbol is kept as-is, matching the Mod's scanner.
pub fn strip_legacy_codes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' || c == '§' {
            if chars.peek().is_some() {
                chars.next();
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// §2.7 — permission first, then the once-per-message cooldown.
fn can_use_function(
    sender: &Player,
    function: &str,
    gate: &Gate,
    cooldown_granted: &mut HashSet<String>,
) -> bool {
    let perm = gate.permission.trim();
    if !perm.is_empty() && !perm.eq_ignore_ascii_case("none") && !sender.has_permission(perm) {
        return false;
    }
    if gate.cooldown_millis <= 0 {
        return true;
    }
    if is_op2(sender.get_permission_level()) {
        return true; // OP level ≥ 2: never consumes a cooldown
    }
    if cooldown_granted.contains(function) {
        return true; // same message, same function → already granted (§2.7.4)
    }
    let key = format!("{}:{function}", sender.get_name().to_ascii_lowercase());
    let mut map = cooldowns().lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    if map.get(&key).is_some_and(|until| *until > now) {
        return false;
    }
    map.insert(key, now + Duration::from_millis(gate.cooldown_millis as u64));
    cooldown_granted.insert(function.to_string());
    true
}

fn is_op2(level: PermissionLevel) -> bool {
    matches!(
        level,
        PermissionLevel::Two | PermissionLevel::Three | PermissionLevel::Four
    )
}

/// The mention pattern must match the default `@? ?(names)` shape: any
/// combination of an optional `@`, one optional space and a literal prefix,
/// then exactly one `(names)` alternation and nothing after it. Returns
/// `(optional_at, optional_space)`.
fn mention_pattern_shape(pattern: &str) -> Option<(bool, bool)> {
    let names_idx = pattern.find("(names)")?;
    if !pattern[names_idx + "(names)".len()..].is_empty() {
        return None; // anything after `(names)` is unsupported
    }
    let mut optional_at = false;
    let mut optional_space = false;
    let mut rest = &pattern[..names_idx];
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix("@?") {
            optional_at = true;
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix(" ?") {
            optional_space = true;
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("\\s?") {
            optional_space = true;
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("\\s*") {
            optional_space = true; // tolerate `\s*` as a one-space optional
            rest = tail;
        } else {
            let c = rest.chars().next()?;
            // Reject regex metacharacters — only literal prefixes are matched.
            if "()[]{}.*+?^$|\\:=<>&\"'".contains(c) {
                return None;
            }
            rest = &rest[c.len_utf8()..];
        }
    }
    Some((optional_at, optional_space))
}

/// Scans `message` for mention matches of the `(names)` alternation. Returns
/// `(start, end, real_name)` for every non-overlapping hit. `names` must be
/// sorted longest-first (substring safety).
fn scan_mention(message: &str, names: &[String], pattern: &str) -> Vec<(usize, usize, String)> {
    let Some((optional_at, optional_space)) = mention_pattern_shape(pattern) else {
        eprintln!(
            "[trchat] function: unsupported Mention pattern '{pattern}' (only the '@? ?(names)' shape is supported) — mentions disabled"
        );
        return Vec::new();
    };
    let bytes = message.as_bytes();
    let starts: Vec<usize> = message.char_indices().map(|(i, _)| i).collect();
    let mut hits = Vec::new();
    let mut k = 0usize;
    while k < starts.len() {
        let i = starts[k];
        let mut at = i;
        if optional_at && bytes.get(at) == Some(&b'@') {
            at += 1;
        }
        if optional_space && bytes.get(at) == Some(&b' ') {
            at += 1;
        }
        let mut matched: Option<(usize, &str)> = None;
        for name in names {
            let nb = name.as_bytes();
            // Only full ASCII-case-insensitive prefix matches of the whole name.
            if at + nb.len() <= bytes.len() && bytes[at..at + nb.len()].eq_ignore_ascii_case(nb) {
                matched = Some((at + nb.len(), name));
                break; // names are longest-first
            }
        }
        if let Some((end, name)) = matched {
            hits.push((i, end, name.to_string()));
            k = starts.partition_point(|&s| s < end);
        } else {
            k += 1;
        }
    }
    hits
}

/// Case-insensitive literal scan (Mention-All `Keys`, `Pattern.quote`d in
/// the Mod). Returns non-overlapping `(start, end)` byte ranges.
fn scan_literal_ci(message: &str, key: &str) -> Vec<(usize, usize)> {
    let kb = key.as_bytes();
    if kb.is_empty() {
        return Vec::new();
    }
    let bytes = message.as_bytes();
    let starts: Vec<usize> = message.char_indices().map(|(i, _)| i).collect();
    let mut hits = Vec::new();
    let mut k = 0usize;
    while k < starts.len() {
        let i = starts[k];
        if i + kb.len() <= bytes.len() && bytes[i..i + kb.len()].eq_ignore_ascii_case(kb) {
            hits.push((i, i + kb.len()));
            k = starts.partition_point(|&s| s < i + kb.len());
        } else {
            k += 1;
        }
    }
    hits
}

/// Builds the styled body component for the broadcast (prefix/suffix handled
/// by the caller). Mention spans become AQUA + hover, Mention-All becomes
/// GOLD + BOLD + hover; plain text between them inherits the surrounding
/// component style.
///
/// `template` is the channel/plain format with an **empty** `{message}`, so
/// the styled body can be appended as a child (§3.1: the Mod builds
/// `Component.empty()` + legacy formatting + `append(messageComponent)`).
/// `TextComponent` is a WIT resource handle whose mutators consume it, so the
/// caller passes the template in and receives a ready-to-send component.
pub fn build_body_component(
    template: &str,
    out: &FunctionOutcome,
    sender_name: &str,
    locale: &str,
) -> TextComponent {
    // Every `TextComponent` mutator in the WIT interface consumes the handle,
    // so each step rebinds instead of borrowing.
    let mut root = TextComponent::from_legacy_string_with_code(template, '&');
    let table = lang::lang().read().unwrap_or_else(|e| e.into_inner());
    let mut cursor = 0usize;
    for span in &out.spans {
        root = root.add_text(&out.body[cursor..span.start]);
        let seg = match &span.kind {
            SpanKind::Mention { target } => {
                let hover = table.format("Function-Mention-Hover", locale, &[sender_name, target]);
                let c = TextComponent::text(&format!("@{target}"))
                    .color_named(NamedColor::Aqua)
                    .hover_show_text(TextComponent::from_legacy_string_with_code(&hover, '&'));
                c
            }
            SpanKind::MentionAll => {
                let hover = table.format("Function-Mention-All-Hover", locale, &[sender_name]);
                let c = TextComponent::text("@所有人")
                    .color_named(NamedColor::Gold)
                    .bold(true)
                    .hover_show_text(TextComponent::from_legacy_string_with_code(&hover, '&'));
                c
            }
        };
        root = root.add_child(seg);
        cursor = span.end;
    }
    root.add_text(&out.body[cursor..])
}

/// §2.9 note — `notifyMention` sends the title/subtitle/actionbar/sound
/// sequence only to players who both were mentioned **and** really received
/// the broadcast; the caller filters by radius/ignored before calling this.
pub fn notify_mentioned(player: &Player, sender_name: &str, locale: &str) {
    let table = lang::lang().read().unwrap_or_else(|e| e.into_inner());
    let notify = table.format("Function-Mention-Notify", locale, &[sender_name]);
    player.show_actionbar(TextComponent::from_legacy_string_with_code(&notify, '&'));
    player.send_title_animation(10, 50, 10);
    let title = table.format("Function-Mention-Title", locale, &[sender_name]);
    player.show_title(TextComponent::from_legacy_string_with_code(&title, '&'));
    let subtitle = table.format("Function-Mention-Subtitle", locale, &[sender_name]);
    player.show_subtitle(TextComponent::from_legacy_string_with_code(&subtitle, '&'));
    // §2.9.1 — `SoundEvents.ANVIL_LAND`, volume 1.0, pitch 2.0.
    player.play_sound(
        pumpkin_plugin_api::player::Sound::BlockAnvilLand,
        pumpkin_plugin_api::player::SoundCategory::Master,
        1.0,
        2.0,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        vec!["Steve".into(), "Alex".into()]
    }

    #[test]
    fn mention_simple() {
        let out = scan_mention("@Steve hello", &names(), "@? ?(names)");
        assert_eq!(out, vec![(0, 6, "Steve".to_string())]);
    }

    #[test]
    fn mention_without_at() {
        let out = scan_mention("hi Alex", &names(), "@? ?(names)");
        // The optional `@? ?` prefix consumes the separating space too.
        assert_eq!(out, vec![(2, 7, "Alex".to_string())]);
    }

    #[test]
    fn mention_longest_first() {
        let mut ns = vec!["Al".to_string(), "Alex".to_string()];
        ns.sort_by(|a, b| b.len().cmp(&a.len()));
        let out = scan_mention("@Alex", &ns, "@? ?(names)");
        assert_eq!(out, vec![(0, 5, "Alex".to_string())]);
    }

    #[test]
    fn mention_multi_and_overlap() {
        // Two mentions separated by text; "Steve" inside a larger word still
        // matches (no word boundary, matching the Mod's regex).
        let out = scan_mention("@Steve X @Alex", &names(), "@? ?(names)");
        assert_eq!(
            out,
            vec![(0, 6, "Steve".to_string()), (9, 14, "Alex".to_string())]
        );
    }

    #[test]
    fn mention_case_insensitive() {
        let out = scan_mention("@steve", &names(), "@? ?(names)");
        assert_eq!(out, vec![(0, 6, "Steve".to_string())]);
    }

    #[test]
    fn mention_unsupported_pattern_skipped() {
        let out = scan_mention("@Steve", &names(), "@? ?\\d+(names)");
        assert!(out.is_empty());
    }

    #[test]
    fn literal_scan_ci_non_overlapping() {
        let out = scan_literal_ci("@all and @ALL and @all", "@all");
        assert_eq!(out, vec![(0, 4), (9, 13), (18, 22)]);
    }

    #[test]
    fn span_offsets_are_byte_ranges() {
        // Ensure the scanner walks char boundaries (Chinese body text).
        let out = scan_mention("你好 @Steve 喵", &names(), "@? ?(names)");
        assert_eq!(out, vec![(7, 13, "Steve".to_string())]);
    }
}
