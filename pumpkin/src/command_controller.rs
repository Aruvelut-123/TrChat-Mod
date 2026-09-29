//! `General.Command-Controller` — the command allow-list guard
//! (`ChatFunctionService.checkCommand`, spec §2.4).
//!
//! The Mod checks the controller before a command reaches the server: a rule
//! that matches may veto the command through its `condition`, and may throttle
//! repeated use through its `cooldown`. Rules are evaluated **in declaration
//! order** and only the first match applies.

use regex::RegexBuilder;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use pumpkin_plugin_api::player::Player;

use crate::condition;
use crate::config::{CommandRule, SharedConfig};

/// Per-player `command:<rule source>` cooldowns (§2.4 step 4).
static COMMAND_COOLDOWNS: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();

fn cooldowns() -> &'static Mutex<HashMap<String, Instant>> {
    COMMAND_COOLDOWNS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// What the caller should do with a submitted command.
pub enum Verdict {
    /// Let the command run.
    Allow,
    /// Cancel it and show `Command-Controller-Deny`.
    Deny,
    /// Cancel it and show `Command-Controller-Cooldown`.
    Cooldown,
}

/// `ChatFunctionService.checkCommand` (spec §2.4).
///
/// `command` is the raw player input (leading `/` optional).
pub fn check_command(player: &Player, command: &str, config: &SharedConfig) -> Verdict {
    let config = config.read();
    let controller = &config.function.command_controller;

    // 1. Controller disabled → allow everything.
    if !controller.enabled {
        return Verdict::Allow;
    }

    // 2. No matching rule → allow.
    let Some(rule) = matching_rule(&controller.rules, command) else {
        return Verdict::Allow;
    };

    // 3. A non-empty condition that evaluates to false denies the command.
    if !rule.condition.is_empty() && !condition::test(&rule.condition, player) {
        return Verdict::Deny;
    }

    // 4. Cooldown — players with `trchat.bypass.cmdcooldown` are exempt and
    //    do not consume the window.
    if rule.cooldown_millis > 0 && !player.has_permission("trchat.bypass.cmdcooldown") {
        if !check_cooldown(
            &player.get_name().to_ascii_lowercase(),
            &format!("command:{}", rule.source),
            rule.cooldown_millis as u64,
        ) {
            return Verdict::Cooldown;
        }
    }

    // 5. Otherwise the command runs.
    Verdict::Allow
}

/// `isCommandManaged` (spec §2.4) — the controller is enabled **and** at least
/// one rule exists. Used for the `/trchat` sub-command availability check.
pub fn is_command_managed(config: &SharedConfig) -> bool {
    let config = config.read();
    config.function.command_controller.enabled && !config.function.command_controller.rules.is_empty()
}

/// `CommandController.matching` (spec §2.4): strip a leading `/`, take the
/// command label, then walk the rules in order.
fn matching_rule<'a>(rules: &'a [CommandRule], command: &str) -> Option<&'a CommandRule> {
    let input = command.trim_start().strip_prefix('/').unwrap_or(command).trim();
    if input.is_empty() {
        return None;
    }
    let label = input.split_whitespace().next().unwrap_or("");

    for rule in rules {
        // The Mod compiles every rule with `CASE_INSENSITIVE` and calls
        // `matcher(subject).matches()` — a *whole-string* match, not a search.
        // Anchoring the pattern reproduces `matches()`; a bad pattern is
        // skipped with a warning rather than aborting the whole list.
        let Ok(re) = RegexBuilder::new(&format!("^(?:{})$", rule.pattern))
            .case_insensitive(true)
            .build()
        else {
            continue;
        };
        let subject = if rule.exact { input } else { label };
        if re.is_match(subject) {
            return Some(rule);
        }
    }
    None
}

/// Records a use of `command:<rule>` and reports whether the call is allowed.
/// A rejected call does **not** refresh the window (same semantics as the
/// mention cooldown).
fn check_cooldown(player_key: &str, key: &str, cooldown_millis: u64) -> bool {
    let now = Instant::now();
    let window = Duration::from_millis(cooldown_millis);
    let mut guard = cooldowns().lock().unwrap_or_else(|e| e.into_inner());
    let entry = format!("{player_key}:{key}");
    match guard.get(&entry) {
        Some(last) if now.duration_since(*last) < window => false,
        _ => {
            guard.insert(entry, now);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(source: &str, pattern: &str, exact: bool, condition: &str, cooldown: i64) -> CommandRule {
        CommandRule {
            source: source.to_string(),
            pattern: pattern.to_string(),
            exact,
            condition: condition.to_string(),
            cooldown_millis: cooldown,
        }
    }

    fn default_rules() -> Vec<CommandRule> {
        vec![
            rule("arasple{exact: true}{condition: perm \"trchat.admin\"}", "arasple", true, "perm \"trchat.admin\"", 0),
            rule("ver(sion)?(s)?{condition: perm \"trchat.admin\"}", "ver(sion)?(s)?", false, "perm \"trchat.admin\"", 0),
            rule("help(s)?{condition: perm *trchat.admin}", "help(s)?", false, "perm *trchat.admin", 0),
            rule("shout{cooldown: 3}", "shout", false, "", 3000),
        ]
    }

    #[test]
    fn leading_slash_is_stripped_and_label_extracted() {
        let rules = default_rules();
        // `ver(sion)?(s)?` matches the label `version` even with arguments.
        let hit = matching_rule(&rules, "/version 1.2").unwrap();
        assert_eq!(hit.pattern, "ver(sion)?(s)?");
        // The label is what is matched, not the whole input.
        assert!(matching_rule(&rules, "/ver abc").is_some());
    }

    #[test]
    fn optional_groups_match_both_forms() {
        let rules = default_rules();
        assert!(matching_rule(&rules, "/ver").is_some());
        assert!(matching_rule(&rules, "/version").is_some());
        assert!(matching_rule(&rules, "/versions").is_some());
        assert!(matching_rule(&rules, "/help").is_some());
        assert!(matching_rule(&rules, "/helps").is_some());
        // `hel` is not `help(s)?`.
        assert!(matching_rule(&rules, "/hel").is_none());
    }

    #[test]
    fn exact_rule_matches_the_whole_input_only() {
        let rules = default_rules();
        // `exact: true` → `matcher(整个输入).matches()`.
        assert!(matching_rule(&rules, "/arasple").is_some());
        assert!(matching_rule(&rules, "arasple").is_some());
        // With arguments the whole-input match fails…
        assert!(matching_rule(&rules, "/arasple now").is_none());
        // …while a non-exact rule still matches on its label.
        assert!(matching_rule(&rules, "/shout hey").is_some());
    }

    #[test]
    fn first_matching_rule_wins_in_order() {
        let rules = vec![
            rule("a{cooldown: 1}", "shout", false, "", 1000),
            rule("b{cooldown: 9}", "shout", false, "", 9000),
        ];
        let hit = matching_rule(&rules, "/shout").unwrap();
        assert_eq!(hit.source, "a{cooldown: 1}");
    }

    #[test]
    fn empty_input_never_matches() {
        let rules = default_rules();
        assert!(matching_rule(&rules, "").is_none());
        assert!(matching_rule(&rules, "   ").is_none());
        assert!(matching_rule(&rules, "/").is_none());
    }

    #[test]
    fn invalid_pattern_is_skipped() {
        let rules = vec![rule("bad", "ver(sion", false, "", 0)];
        assert!(matching_rule(&rules, "/version").is_none());
    }

    #[test]
    fn cooldown_window_rejects_then_expires() {
        let key = "cmdtest-player";
        assert!(check_cooldown(key, "command:shout{cooldown: 3}", 50));
        // Inside the window the second call is rejected…
        assert!(!check_cooldown(key, "command:shout{cooldown: 3}", 50));
        std::thread::sleep(Duration::from_millis(60));
        // …and once it elapses the command is allowed again.
        assert!(check_cooldown(key, "command:shout{cooldown: 3}", 50));
    }

    #[test]
    fn cooldown_keys_are_per_player_and_per_rule() {
        assert!(check_cooldown("cmdtest-a", "command:shout", 60_000));
        // A different rule is an independent window…
        assert!(check_cooldown("cmdtest-a", "command:yell", 60_000));
        // …and so is a different player.
        assert!(check_cooldown("cmdtest-b", "command:shout", 60_000));
    }
}
