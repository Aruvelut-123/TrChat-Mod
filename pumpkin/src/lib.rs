//! TrChat — experimental PumpkinMC support.
//!
//! This crate is a WASM component plugin for the [Pumpkin](https://pumpkinmc.org)
//! Minecraft server. It ports the *local* chat core of TrChat (the Bukkit plugin):
//!
//! * intercepts player chat through the `player-chat` event,
//! * renders the message with the configured format,
//! * broadcasts the rendered message to all online players.
//!
//! Cross-server (Redis) messaging is a documented follow-up; the crate already
//! declares the network permissions the future proxy code will need.

mod block_filter;
mod chat;
mod clock;
mod cloud;
mod command_controller;
mod commands;
mod condition;
mod config;
mod diag;
mod filter;
mod functions;
mod http;
mod lang;
mod perms;
mod placeholder;
mod playerdata;
mod private_msg;
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
        crate::diag::info(format!(
            "TrChat {} loaded (Pumpkin WASM port)",
            crate::updater::CURRENT_VERSION
        ));
        Ok(())
    }
}

register_plugin!(TrChatPlugin);
