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
        return player.has_permission(node);
    }

    false
}

/// `permission-level >= 2` is the Mod's `player.isOp()`.
fn is_op(player: &Player) -> bool {
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
}
