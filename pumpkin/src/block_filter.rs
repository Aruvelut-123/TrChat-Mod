//! Sign and anvil filtering — `filter.yml`'s `Enable.Sign` / `Enable.Anvil`.
//!
//! The Mod wires this through `ListenerSignChange` and `ListenerAnvilChange`
//! (Bukkit `SignChangeEvent` / `PrepareAnvilEvent`, both at priority HIGHEST):
//! every non-blank sign line and the anvil rename text go through
//! `FilterManager.filter(...).filtered`. The local port's `FilterService`
//! additionally reports a blocked anvil name with `Filter-Anvil-Blocked`
//! (`FilterService.checkAnvil`, OP level 2 exempt).
//!
//! Host mapping (verified against the pinned `pumpkin-plugin-api`):
//!
//! * `SignChangeEvent` (`event.wit:1335-1340`) carries `lines: list<string>`
//!   and the host reads that list back after the handlers run
//!   (`wasm_host/wit/v0_1/events/block.rs`, `ToFromWasmEvent for SignChangeEvent`),
//!   so a rewritten line reaches the sign.
//! * `PrepareAnvilEvent` (`event.wit:1752-1756`) carries `rename-text` and
//!   `repair-cost` only — no result item, no display-name component and no
//!   `cancelled` flag. The rename text is read back, so the typed name is what
//!   this port filters; the result item's display name, the cancelled
//!   `Anvil-Edit-No-Permission` branch and the component-only `Color.Anvil` /
//!   `Simple-Component.Anvil` passes have no host surface in this API.
//!
//! As in the chat pipeline, the filter itself runs for operators too: the Mod's
//! `trchat.bypass.filter` exemption is not part of this port (the node is absent
//! from `commands::register_permissions`), while the `Filter-Anvil-Blocked`
//! notification keeps the local port's OP-2 bypass.

use pumpkin_plugin_api::{
    events::{EventData, EventHandler, EventPriority, PrepareAnvilEvent, SignChangeEvent},
    text::TextComponent,
    Context, Server,
};

use crate::config::{FilterConfig, SharedConfig};
use crate::filter::TextFilter;
use crate::lang;

/// Registers the sign and anvil filter handlers.
///
/// Both are blocking and run at `EventPriority::Highest`, matching the Mod's
/// `@SubscribeEvent(priority = EventPriority.HIGHEST)` subscriptions.
pub fn register(context: &Context) -> Result<(), String> {
    let config = crate::config::global_config().clone();
    context
        .register_event_handler::<SignChangeEvent, SignChangeHandler>(
            SignChangeHandler {
                config: config.clone(),
            },
            EventPriority::Highest,
            true,
        )
        .map_err(|e| e.to_string())?;
    context
        .register_event_handler::<PrepareAnvilEvent, AnvilHandler>(
            AnvilHandler { config },
            EventPriority::Highest,
            true,
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Builds the `filter.yml` profile exactly as the chat pipeline does (local
/// words plus the cloud thesaurus).
fn text_filter(filter: &FilterConfig) -> TextFilter {
    crate::filter::text_filter(filter)
}

/// Filters every non-blank sign line (`ListenerSignChange.onSignChange`).
///
/// `Enable.Sign` gates the whole pass; blank lines are left untouched, as the
/// Mod `continue`s on `origin.isBlank()`.
pub fn filter_sign_lines(filter: &FilterConfig, lines: &[String]) -> Vec<String> {
    if !filter.sign_enabled {
        return lines.to_vec();
    }
    let sensitive = text_filter(filter);
    if !sensitive.is_active() {
        return lines.to_vec();
    }
    lines
        .iter()
        .map(|line| {
            if line.trim().is_empty() {
                line.clone()
            } else {
                sensitive.filter(line)
            }
        })
        .collect()
}

/// Filters an anvil rename and reports the number of matched words
/// (`ListenerAnvilChange.onAnvilCraft` + `FilterService.checkAnvil`).
///
/// `Enable.Anvil` gates the pass; the returned count is the filter's
/// `matches()`, which drives the `Filter-Anvil-Blocked` notification.
pub fn filter_anvil_name(filter: &FilterConfig, name: &str) -> (String, usize) {
    if !filter.anvil_enabled {
        return (name.to_string(), 0);
    }
    let sensitive = text_filter(filter);
    if !sensitive.is_active() {
        return (name.to_string(), 0);
    }
    sensitive.filter_with_count(name)
}

/// `SignChangeEvent` — rewrites the edited sign lines in place.
struct SignChangeHandler {
    config: SharedConfig,
}

impl EventHandler<SignChangeEvent> for SignChangeHandler {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<SignChangeEvent>,
    ) -> EventData<SignChangeEvent> {
        let config = self.config.read();
        event.lines = filter_sign_lines(config.filter_config(), &event.lines);
        event
    }
}

/// `PrepareAnvilEvent` — filters the typed rename text and reports a blocked
/// name to the player.
struct AnvilHandler {
    config: SharedConfig,
}

impl EventHandler<PrepareAnvilEvent> for AnvilHandler {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<PrepareAnvilEvent>,
    ) -> EventData<PrepareAnvilEvent> {
        let config = self.config.read();
        let (name, matches) = filter_anvil_name(config.filter_config(), &event.rename_text);
        if matches > 0 && !crate::condition::is_op(&event.player) {
            let locale = event.player.get_locale();
            let text = lang::lang()
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .format("Filter-Anvil-Blocked", &locale, &[]);
            event.player.send_system_message(
                TextComponent::from_legacy_string_with_code(&text, '&'),
                false,
            );
        }
        event.rename_text = name;
        event
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(local: &[&str], chat: bool, sign: bool, anvil: bool) -> FilterConfig {
        FilterConfig {
            chat_enabled: chat,
            sign_enabled: sign,
            anvil_enabled: anvil,
            cloud_enabled: false,
            cloud_urls: Vec::new(),
            cloud_ignored: Vec::new(),
            local_words: local.iter().map(|w| (*w).to_string()).collect(),
            ignored_punctuations: Vec::new(),
            white_list: Vec::new(),
            replacement: '*',
        }
    }

    #[test]
    fn sign_lines_are_filtered_when_enabled() {
        let filter = profile(&["badword"], true, true, true);
        let lines = vec![
            "hello".to_string(),
            "a badword here".to_string(),
            "   ".to_string(),
        ];
        let out = filter_sign_lines(&filter, &lines);
        assert_eq!(out[0], "hello");
        assert_eq!(out[1], "a ******* here");
        // Blank lines stay untouched (`origin.isBlank()` → continue).
        assert_eq!(out[2], "   ");
    }

    #[test]
    fn sign_lines_are_untouched_when_disabled_or_inactive() {
        let disabled = profile(&["badword"], true, false, true);
        let lines = vec!["a badword here".to_string()];
        assert_eq!(filter_sign_lines(&disabled, &lines), lines);

        // No local words at all → `TextFilter::is_active()` is false.
        let empty = profile(&[], true, true, true);
        assert_eq!(filter_sign_lines(&empty, &lines), lines);
    }

    #[test]
    fn anvil_names_are_filtered_and_counted() {
        let filter = profile(&["badword"], true, true, true);
        assert_eq!(filter_anvil_name(&filter, "a badword"), ("a *******".to_string(), 1));
        assert_eq!(filter_anvil_name(&filter, "clean"), ("clean".to_string(), 0));
    }

    #[test]
    fn anvil_bypass_leaves_the_name_alone() {
        let disabled = profile(&["badword"], true, true, false);
        assert_eq!(
            filter_anvil_name(&disabled, "a badword"),
            ("a badword".to_string(), 0)
        );
    }
}
