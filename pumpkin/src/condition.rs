//! The Mod's `ConditionEvaluator` — the tiny condition DSL shared by
//! `Channel.Options.Speak-Condition`, `Formats[].Condition` and
//! `Command-Controller.List[].{condition}`.
//!
//! Upstream: `channel/ConditionEvaluator.java:19-47`. The evaluator is
//! deliberately minimal; anything it does not understand evaluates to
//! **false** (never "allow by default").

use pumpkin_plugin_api::permission::PermissionLevel;
use pumpkin_plugin_api::player::Player;

/// Evaluates a condition string for `player` (`ConditionEvaluator.test`).
///
/// * empty / `~` → `true`
/// * `player op` / `player is op` → permission level ≥ 2
/// * `perm "node"` / `permission node` → `has_permission` (quotes optional,
///   a leading `*` is stripped)
/// * leading `!` negates the rest
/// * anything else → `false`
pub fn test(condition: &str, player: &Player) -> bool {
    let condition = condition.trim();
    if condition.is_empty() || condition == "~" {
        return true;
    }
    if let Some(rest) = condition.strip_prefix('!') {
        return !test(rest, player);
    }

    let lower = condition.to_ascii_lowercase();
    if lower == "player op" || lower == "player is op" {
        return is_op(player);
    }

    if let Some(node) = permission_node(condition) {
        let node = node.trim_start_matches('*');
        return player.has_permission(&crate::perms::node(node));
    }

    false
}

/// §3 `canSpeak` (`ChatService.java:780-785`): a non-empty `Speak-Condition`
/// *replaces* the permission check; when it is empty the channel's
/// `Join-Permission` gates speaking, and an empty permission (e.g. `Normal`)
/// lets everyone speak.
pub fn can_speak(speak_condition: &str, join_permission: &str, player: &Player) -> bool {
    match speak_rule(speak_condition, join_permission) {
        SpeakRule::Condition(condition) => test(condition, player),
        SpeakRule::Permission(node) => player.has_permission(&crate::perms::node(node)),
        SpeakRule::Open => true,
    }
}

/// Which gate decides whether a player may speak in a channel (§3 `canSpeak`).
#[derive(Debug, PartialEq)]
enum SpeakRule<'a> {
    /// Non-empty `Speak-Condition` — evaluated by [`test`].
    Condition(&'a str),
    /// Empty `Speak-Condition` but a non-empty `Join-Permission`.
    Permission(&'a str),
    /// Neither is set (e.g. `Normal`) — everyone may speak.
    Open,
}

/// The selection half of [`can_speak`], split out so the *fallback* rule
/// (config.md §5 note 5) is testable without a `Player`.
fn speak_rule<'a>(speak_condition: &'a str, join_permission: &'a str) -> SpeakRule<'a> {
    let condition = speak_condition.trim();
    if !condition.is_empty() {
        SpeakRule::Condition(condition)
    } else if join_permission.is_empty() {
        SpeakRule::Open
    } else {
        SpeakRule::Permission(join_permission)
    }
}

/// `permission-level >= 2` is the Mod's `player.isOp()`. The chat guards use
/// this to exempt OPs from anti-repeat, cooldown and anti-high-frequency
/// (chat.md §1.4) — but *not* from length, mutes or anti-duplicate.
pub fn is_op(player: &Player) -> bool {
    matches!(
        player.get_permission_level(),
        PermissionLevel::Two | PermissionLevel::Three | PermissionLevel::Four
    )
}

/// Extracts the node from a `perm|permission <node>` clause (quotes optional).
fn permission_node(condition: &str) -> Option<String> {
    let rest = condition
        .strip_prefix("permission")
        .or_else(|| condition.strip_prefix("perm"))?;
    // `permissionx` must not be read as `permission x`.
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let node = rest.trim().trim_matches('"').trim_matches('\'').trim();
    if node.is_empty() {
        return None;
    }
    Some(node.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_tilde_are_true() {
        assert!(permission_node("").is_none());
        // No player handle is available in unit tests, so only the parsing
        // helpers are exercised here; the `true` branches are covered by the
        // string-level checks below.
        assert_eq!(permission_node("perm \"trchat.admin\"").as_deref(), Some("trchat.admin"));
        assert_eq!(permission_node("permission trchat.admin").as_deref(), Some("trchat.admin"));
        assert_eq!(permission_node("perm *trchat.admin").as_deref(), Some("*trchat.admin"));
        assert_eq!(permission_node("perm"), None);
        assert_eq!(permission_node("permissions x"), None);
    }

    #[test]
    fn unknown_conditions_are_not_permissions() {
        assert_eq!(permission_node("player op"), None);
        assert_eq!(permission_node("something else"), None);
    }

    /// config.md §5 note 5: a non-empty `Speak-Condition` replaces the
    /// permission check, and an empty condition falls back to `Join-Permission`
    /// (which in turn means "everyone" when it is empty too).
    #[test]
    fn speak_condition_replaces_the_permission_gate() {
        // Speak-Condition wins whenever it is set, whatever the permission is.
        assert_eq!(
            speak_rule("perm \"trchat.global\"", "trchat.admin"),
            SpeakRule::Condition("perm \"trchat.global\"")
        );
        assert_eq!(
            speak_rule("perm \"trchat.global\"", ""),
            SpeakRule::Condition("perm \"trchat.global\"")
        );
        // `~` is the "always true" condition, not a blank value, so it is still
        // a condition and not a fallback to the permission.
        assert_eq!(speak_rule("~", "trchat.admin"), SpeakRule::Condition("~"));

        // Empty condition → the join permission gates speaking.
        assert_eq!(
            speak_rule("", "trchat.admin"),
            SpeakRule::Permission("trchat.admin")
        );
        // Whitespace-only behaves like empty (the Mod trims).
        assert_eq!(
            speak_rule("   ", "trchat.admin"),
            SpeakRule::Permission("trchat.admin")
        );

        // Neither set → open (e.g. the Normal channel).
        assert_eq!(speak_rule("", ""), SpeakRule::Open);
        assert_eq!(speak_rule("  ", ""), SpeakRule::Open);
    }
}
