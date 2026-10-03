//! Permission-node spelling.
//!
//! The host is strict about how a plugin names its permission nodes, and the
//! rules differ between *registration* and *lookup*:
//!
//! * `Context::register_permission` (host `plugin/api/context.rs:278-291`)
//!   **rejects** any node that does not start with `{plugin_name}:` — and
//!   `pumpkin_util::permission::PermissionRegistry::register_permission`
//!   rejects a node that is registered twice (`pumpkin-util/src/permission.rs:90-99`).
//! * `Context::register_command` (host `plugin/api/context.rs`) qualifies a
//!   *bare* requirement itself (`format!("{name}:{permission}")`) and leaves a
//!   requirement that already contains `:` untouched, so the command tree
//!   resolves against the namespaced spelling.
//! * `PermissionManager::has_permission` (`pumpkin-util/src/permission.rs:330-388`)
//!   looks the node up **exactly** and denies anything that is not registered.
//!
//! The Mod, however, writes bare nodes in its YAML (`perm "trchat.global"`,
//! channel `Join-Permission: 'trchat.private'`,
//! `Permission: 'trchat.function.mentionall'`), and the port's own literals are
//! bare as well. Every string that reaches a host permission lookup therefore
//! goes through [`node`] first, which applies the same qualification the host
//! applies to command requirements.

/// The plugin name the host namespaces permission nodes with.
pub const PLUGIN_NAMESPACE: &str = "trchat";

/// Qualifies `raw` the way the host's permission registry expects it.
///
/// A string that already carries a namespace (`trchat:trchat.mute`) is returned
/// unchanged, so the `PERM_*` constants in [`crate::commands`] — which are
/// already qualified — round-trip through this function untouched.
#[must_use]
pub fn node(raw: &str) -> String {
    if raw.contains(':') {
        raw.to_string()
    } else {
        format!("{PLUGIN_NAMESPACE}:{raw}")
    }
}

#[cfg(test)]
mod tests {
    use super::node;

    #[test]
    fn bare_nodes_gain_the_plugin_namespace() {
        assert_eq!(node("trchat.global"), "trchat:trchat.global");
        assert_eq!(node("trchat.bypass.repeat"), "trchat:trchat.bypass.repeat");
        assert_eq!(node("trchat.color.0"), "trchat:trchat.color.0");
    }

    #[test]
    fn qualified_nodes_are_left_alone() {
        for already in [
            "trchat:trchat.use",
            "trchat:trchat.admin",
            "trchat:trchat.command.channel.other",
            "other:plugin.node",
        ] {
            assert_eq!(node(already), already);
        }
    }
}
