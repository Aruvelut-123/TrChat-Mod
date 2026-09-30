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

mod chat;
mod clock;
mod command_controller;
mod commands;
mod condition;
mod config;
mod filter;
mod functions;
mod lang;
mod placeholder;
mod playerdata;
mod private_msg;
mod snapshot;
mod special;

use pumpkin_plugin_api::{
    permissions::{
        FS_READ_DATA, FS_WRITE_DATA, NETWORK_DNS, NETWORK_LOOPBACK, NETWORK_OUTBOUND, NETWORK_TCP,
        NETWORK_TCP_CONNECT,
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
            version: env!("CARGO_PKG_VERSION").to_string(),
            authors: vec!["TrChat Team".to_string()],
            description: "TrChat experimental support for PumpkinMC".to_string(),
            dependencies: vec![],
            permissions: vec![
                NETWORK_TCP.to_string(),
                NETWORK_TCP_CONNECT.to_string(),
                NETWORK_DNS.to_string(),
                NETWORK_LOOPBACK.to_string(),
                NETWORK_OUTBOUND.to_string(),
                FS_READ_DATA.to_string(),
                FS_WRITE_DATA.to_string(),
            ],
        }
    }

    fn on_load(&self, context: Context) -> Result<(), String> {
        // `%server_uptime%` counts from the plugin load (≈ server start), the
        // closest the sandbox gets to the JVM uptime the Mod reports.
        crate::clock::mark_start();
        // Commands are registered before the chat pipeline so both borrow
        // the context without conflict.
        crate::commands::register_commands(&context);
        ChatManager::init(context)
    }
}

register_plugin!(TrChatPlugin);
