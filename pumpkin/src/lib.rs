//! TrChat — experimental PumpkinMC support.
//!
//! This crate is a WASM component plugin for the [Pumpkin](https://pumpkinmc.org)
//! Minecraft server. It ports the *local* chat core of TrChat (the Bukkit plugin):
//!
//! * intercepts player chat through the `player-chat` event,
//! * renders the message with the configured format,
//! * broadcasts the rendered message to all online players,
//! * relays chat, private messages, the player list, the global mute and
//!   language notices to other TrChat servers over Redis or the Bukkit/Velocity
//!   plugin-message bridge (`redis::start` / `proxy::start`), in the wire format
//!   the Bukkit/NeoForge Mod publishes.
//!
//! The crate declares the network permissions the Redis transport needs.

mod block_filter;
mod chat;
mod clock;
mod cloud;
mod command_controller;
mod commands;
mod condition;
mod config;
mod datasource;
mod diag;
mod filter;
mod functions;
mod http;
mod lang;
mod perms;
mod placeholder;
mod playerdata;
mod private_msg;
mod proxy;
mod redis;
mod resp;
mod snapshot;
mod special;
mod sync;
mod updater;

use pumpkin_plugin_api::{
    permissions::{
        FS_READ_DATA, FS_WRITE_DATA, HTTP_OUTBOUND, NETWORK_DNS, NETWORK_LOOPBACK,
        NETWORK_OUTBOUND, NETWORK_TCP, NETWORK_TCP_CONNECT,
    },
    register_plugin, Context, Plugin, PluginMetadata,
};

use crate::chat::ChatManager;

/// The plugin entry type. See [`Plugin`] for the lifecycle.
pub struct TrChatPlugin;

impl Plugin for TrChatPlugin {
    fn new() -> Self {
        Self
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "trchat".to_string(),
            // The version of the Mod this port tracks (`mod_version` in the
            // repository `gradle.properties`, injected by `build.rs`), not the
            // crate's own — the host lists this value in `/plugins`.
            version: crate::updater::CURRENT_VERSION.to_string(),
            authors: vec!["TrChat Team".to_string()],
            description: "TrChat experimental support for PumpkinMC".to_string(),
            dependencies: vec![],
            permissions: vec![
                NETWORK_TCP.to_string(),
                NETWORK_TCP_CONNECT.to_string(),
                NETWORK_DNS.to_string(),
                NETWORK_LOOPBACK.to_string(),
                NETWORK_OUTBOUND.to_string(),
                // The `updates:` checker GETs the GitHub releases API through the
                // host's `wasi:http` client; without this node the host answers
                // `HttpRequestDenied` (`wasm_host/state.rs:664`).
                HTTP_OUTBOUND.to_string(),
                FS_READ_DATA.to_string(),
                FS_WRITE_DATA.to_string(),
            ],
        }
    }

    fn on_load(&self, context: Context) -> Result<(), String> {
        // `%server_uptime%` counts from the plugin load (≈ server start), the
        // closest the sandbox gets to the JVM uptime the Mod reports.
        crate::clock::mark_start();
        // The configuration must be installed *before* the command tree is
        // built: registering commands reads the process-wide handle (the
        // `/global`, `/all`, … aliases come from `Bindings.Command`), and
        // `global_config()` initialises a **default** snapshot on first use.
        // That used to consume the `OnceLock`, after which `init_global` could
        // never install the loaded configuration — a real server showed the
        // whole command surface (and the update checker) on defaults.
        ChatManager::init(&context)?;
        crate::commands::register_commands(&context);
        // `filter.yml`'s `Enable.Sign` / `Enable.Anvil` ride the blocking
        // `SignChangeEvent` / `PrepareAnvilEvent` (the Mod's two listeners); the
        // handlers read the shared configuration, so this must follow
        // `ChatManager::init`.
        crate::block_filter::register(&context)?;
        // Last: the checker reads the global config `ChatManager::init` seeds.
        crate::updater::start(&context)?;
        // Cross-server chat reads the same configuration.  Both transports are
        // registered so `/trchat reload` can enable either one without a plugin
        // restart; the chat path prefers Redis and falls back to plugin messages.
        crate::proxy::start(&context)?;
        crate::redis::start(&context)?;
        // Player-data persistence: restores chat state on join, stores it on
        // leave (`datasource.yml` semantics; file-backed execution layer).
        crate::playerdata::register(&context)?;
        crate::diag::info(format!(
            "TrChat {} loaded (Pumpkin WASM port)",
            crate::updater::CURRENT_VERSION
        ));
        Ok(())
    }

    fn on_unload(&self, _context: Context) -> Result<(), String> {
        // `ModerationService.close` — persist every online player before the
        // plugin goes away.
        crate::playerdata::flush_all();
        Ok(())
    }
}

register_plugin!(TrChatPlugin);
