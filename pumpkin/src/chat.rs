//! Chat interception, rendering and broadcast — the local TrChat core ported to Pumpkin.

use pumpkin_plugin_api::{
    events::{player::PlayerChatEvent, EventData, EventHandler, EventPriority},
    text::TextComponent,
    Context, Server,
};

use crate::config::{SharedConfig, TrChatConfig};

/// Owns chat wiring for the plugin.
pub struct ChatManager;

impl ChatManager {
    /// Loads the configuration and registers the chat event handler.
    pub fn init(context: Context) -> Result<(), String> {
        let config = SharedConfig::load(&context)?;
        context
            .register_event_handler::<PlayerChatEvent, ChatHandler>(
                ChatHandler { config },
                EventPriority::High,
                true,
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Handles `PlayerChatEvent`: renders the message according to the format and
/// broadcasts it to every online player (the experimental Pumpkin port of the
/// TrChat local chat pipeline).
struct ChatHandler {
    config: SharedConfig,
}

impl EventHandler<PlayerChatEvent> for ChatHandler {
    fn handle(
        &self,
        server: Server,
        mut event: EventData<PlayerChatEvent>,
    ) -> EventData<PlayerChatEvent> {
        let name = event.player.get_name();
        let raw_message = event.message.clone();

        // Phase 1 (this branch, experimental):
        //   render + broadcast to all online players.
        // Phase 2 (roadmap):
        //   channel prefix routing (#global / @local), /msg private chat,
        //   and Redis cross-server relay (network.* permissions pre-declared).
        // Note: TextComponent is a WIT handle (not Clone), so we render per player.
        for player in server.get_all_players() {
            let text = render(
                &self.config.0.read().unwrap_or_else(|e| e.into_inner()),
                &name,
                &raw_message,
            );
            let _ = player.send_system_message(text, false);
        }

        // Suppress the server's own formatting so only our rendered message shows.
        event.cancelled = true;
        event.message = String::new();
        event.recipients = Vec::new();
        event
    }
}

/// Builds the displayed chat component from the configured template.
///
/// The template supports `&` color codes (parsed via the legacy string parser)
/// plus the `{player}` / `{message}` placeholders.
fn render(config: &TrChatConfig, name: &str, message: &str) -> TextComponent {
    let template = placeholders(&config.format, name, message);
    TextComponent::from_legacy_string_with_code(&template, '&')
}

/// Replaces `{player}` / `{message}` placeholders in the format template.
fn placeholders(template: &str, name: &str, message: &str) -> String {
    template
        .replace("{player}", name)
        .replace("{message}", message)
}