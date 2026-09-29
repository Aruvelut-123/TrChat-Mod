//! Commands — the Bukkit v2 command surface ported to Pumpkin.
//!
//! Registered command tree (permissions mirror the upstream plugin):
//!
//! * `/trchat reload`        — re-read `config.json` from disk (`trchat.admin`)
//! * `/trchat version`       — print the plugin version (`trchat.use`)
//! * `/trchat muteall`       — toggle the global chat mute (`trchat.admin`)
//! * `/trchat mute <player>` — mute a player (`trchat.admin`)
//! * `/trchat unmute <player>` — unmute a player (`trchat.admin`)
//! * `/trchat ignore <player>` — toggle ignoring a player (`trchat.use`)
//! * `/trchat channel <id>`  — switch the active channel (`trchat.use`)
//! * `/trchat view <snapshot>` — open a read-only inventory snapshot (§2.11)
//! * `/channel <id>`         — alias of `trchat channel`
//! * `/msg <target> <msg>`   — private message (`tell` alias, `trchat.use`)
//!
//! All handlers share the session state from [`crate::playerdata`] and the
//! process-wide config handle from [`crate::config`]; they never build the
//! chat pipeline themselves (that stays in [`crate::chat`]).

use pumpkin_plugin_api::{
    command::{
        Arg, ArgumentType, Command, CommandError, CommandNode, CommandSender, ConsumedArgs,
        StringType,
    },
    commands::CommandHandler,
    gui::Gui,
    text::TextComponent,
    Context, ItemStack, Screen, Server,
};

use crate::config;
use crate::lang;
use crate::playerdata::SessionPlayers;

/// Permission of ordinary chat users (channel switching, ignore, /msg).
const PERM_USE: &str = "trchat.use";
/// Permission of administrators (reload, mute, muteall).
const PERM_ADMIN: &str = "trchat.admin";

/// Registers every TrChat command with the given context.
///
/// Called from [`crate::TrChatPlugin::on_load`] *before* the chat pipeline is
/// initialized, so the borrow of `context` does not outlive the event handler
/// registration done by [`crate::chat::ChatManager::init`].
pub fn register_commands(context: &Context) {
    // ---- /trchat ----
    let trchat = Command::new(
        &[String::from("trchat")],
        "TrChat management and chat commands",
    )
    .then(CommandNode::literal("reload").execute(ReloadCommand))
    .then(CommandNode::literal("version").execute(VersionCommand))
    .then(CommandNode::literal("muteall").execute(MuteAllCommand))
    .then(
        CommandNode::literal("mute").then(
            CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
                .execute(MuteCommand),
        ),
    )
    .then(
        CommandNode::literal("unmute").then(
            CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
                .execute(UnmuteCommand),
        ),
    )
    .then(
        CommandNode::literal("ignore").then(
            CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
                .execute(IgnoreCommand),
        ),
    )
    .then(
        CommandNode::literal("channel").then(
            CommandNode::argument("name", &ArgumentType::String(StringType::SingleWord))
                .execute(ChannelCommand),
        ),
    )
    // §2.11 — `/trchat view <snapshot>` opens the read-only container view.
    .then(
        CommandNode::literal("view").then(
            CommandNode::argument("snapshot", &ArgumentType::String(StringType::SingleWord))
                .execute(ViewCommand),
        ),
    );
    context.register_command(trchat, PERM_USE);

    // ---- /channel <name> ----
    let channel = Command::new(
        &[String::from("channel")],
        "Switch your active chat channel",
    )
    .then(
        CommandNode::argument("name", &ArgumentType::String(StringType::SingleWord))
            .execute(ChannelCommand),
    );
    context.register_command(channel, PERM_USE);

    // ---- /msg <target> <message> ----
    let msg = Command::new(
        &[String::from("msg"), String::from("tell")],
        "Send a private message to a player",
    )
    .then(
        CommandNode::argument("target", &ArgumentType::String(StringType::SingleWord)).then(
            CommandNode::argument("message", &ArgumentType::String(StringType::Greedy))
                .execute(MsgCommand),
        ),
    );
    context.register_command(msg, PERM_USE);
}

/// Extracts a plain string argument (`Arg::Simple`/`Arg::Msg`).
fn arg_string(args: &ConsumedArgs, key: &str) -> Option<String> {
    match args.get_value(key) {
        Arg::Simple(s) | Arg::Msg(s) => Some(s),
        _ => None,
    }
}

/// Sends a legacy-coloured feedback line to the command sender.
fn send(sender: &CommandSender, text: &str) {
    let _ = sender.send_system_message(TextComponent::from_legacy_string_with_code(text, '&'));
}

/// Resolves a locale-aware message by key with positional args.
///
/// `CommandSender::get_locale()` returns the WIT `locale` enum (a large
/// Minecraft locale tag set) which this port does not enumerate; an empty
/// locale string makes [`Lang::get`] fall back to the configured default
/// language, mirroring the server-side default of the upstream plugin.
fn message(key: &str, sender: &CommandSender, args: &[&str]) -> String {
    let _ = sender.get_locale();
    lang::lang()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .format(key, "", args)
}

/// `/trchat reload` — swap the process-wide config from disk.
struct ReloadCommand;

impl CommandHandler for ReloadCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        if !sender.has_permission(&server, PERM_ADMIN) {
            send(&sender, "&cYou do not have permission to use this command.");
            return Ok(0);
        }
        match config::reload_global() {
            Ok(()) => send(&sender, "&a[TrChat] Configuration reloaded."),
            Err(e) => send(&sender, &format!("&c[TrChat] Reload failed: {e}")),
        }
        Ok(0)
    }
}

/// `/trchat version`.
struct VersionCommand;

impl CommandHandler for VersionCommand {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        send(
            &sender,
            &format!(
                "&a[TrChat] TrChat v{} (Pumpkin WASM port)",
                env!("CARGO_PKG_VERSION")
            ),
        );
        Ok(0)
    }
}

/// `/trchat muteall` — toggle the global mute.
struct MuteAllCommand;

impl CommandHandler for MuteAllCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        if !sender.has_permission(&server, PERM_ADMIN) {
            send(&sender, "&cYou do not have permission to use this command.");
            return Ok(0);
        }
        let mut players = SessionPlayers::global()
            .write()
            .unwrap_or_else(|e| e.into_inner());
        let next = !players.is_global_muted();
        players.set_global_muted(next);
        drop(players);
        if next {
            send(&sender, "&c[TrChat] Chat has been globally muted.");
        } else {
            send(&sender, "&a[TrChat] Chat is no longer globally muted.");
        }
        Ok(0)
    }
}

/// `/trchat mute <player>` — mute one player.
struct MuteCommand;

impl CommandHandler for MuteCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        if !sender.has_permission(&server, PERM_ADMIN) {
            send(&sender, "&cYou do not have permission to use this command.");
            return Ok(0);
        }
        let Some(name) = arg_string(&args, "player") else {
            send(&sender, "&cUsage: /trchat mute <player>");
            return Ok(0);
        };
        let mut players = SessionPlayers::global()
            .write()
            .unwrap_or_else(|e| e.into_inner());
        match players.state_mut(&name) {
            Some(state) => {
                state.muted = true;
                send(&sender, &format!("&a[TrChat] Muted {name}."));
            }
            None => send(&sender, &format!("&c[TrChat] {name} is not online.")),
        }
        Ok(0)
    }
}

/// `/trchat unmute <player>` — unmute one player.
struct UnmuteCommand;

impl CommandHandler for UnmuteCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        if !sender.has_permission(&server, PERM_ADMIN) {
            send(&sender, "&cYou do not have permission to use this command.");
            return Ok(0);
        }
        let Some(name) = arg_string(&args, "player") else {
            send(&sender, "&cUsage: /trchat unmute <player>");
            return Ok(0);
        };
        let mut players = SessionPlayers::global()
            .write()
            .unwrap_or_else(|e| e.into_inner());
        match players.state_mut(&name) {
            Some(state) => {
                state.muted = false;
                send(&sender, &format!("&a[TrChat] Unmuted {name}."));
            }
            None => send(&sender, &format!("&c[TrChat] {name} is not online.")),
        }
        Ok(0)
    }
}

/// `/trchat ignore <player>` — toggle ignoring a player.
struct IgnoreCommand;

impl CommandHandler for IgnoreCommand {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let me = sender.get_name();
        let Some(target) = arg_string(&args, "player") else {
            send(&sender, "&cUsage: /trchat ignore <player>");
            return Ok(0);
        };
        if target.eq_ignore_ascii_case(&me) {
            send(&sender, "&cYou cannot ignore yourself.");
            return Ok(0);
        }
        let mut players = SessionPlayers::global()
            .write()
            .unwrap_or_else(|e| e.into_inner());
        match players.state_mut(&me) {
            Some(state) => {
                let lower = target.to_ascii_lowercase();
                if state.ignored.remove(&lower) {
                    send(
                        &sender,
                        &format!("&a[TrChat] You are no longer ignoring {target}."),
                    );
                } else {
                    state.ignored.insert(lower);
                    send(
                        &sender,
                        &format!("&a[TrChat] You are now ignoring {target}."),
                    );
                }
            }
            None => send(&sender, "&c[TrChat] Your chat session is not ready yet."),
        }
        Ok(0)
    }
}

/// `/trchat channel <name>` / `/channel <name>` — switch the active channel.
struct ChannelCommand;

impl CommandHandler for ChannelCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let me = sender.get_name();
        let config = config::global_config();
        // §2.4 — switching channels is a Command-Controller-managed action;
        // with the controller off (or ruleless) the sub-command is unavailable.
        if !crate::command_controller::is_command_managed(&config) {
            send(
                &sender,
                &message("Command-Controller-Disabled", &sender, &["channel"]),
            );
            return Ok(0);
        }
        let Some(name) = arg_string(&args, "name") else {
            // List the available channels when no argument is consumed.
            let ids: Vec<String> = config
                .read()
                .channels()
                .iter()
                .map(|c| c.id.clone())
                .collect();
            send(
                &sender,
                &format!("&a[TrChat] Available channels: {}", ids.join(", ")),
            );
            return Ok(0);
        };
        let guard = config.read();
        let Some(channel) = guard.channel_by_id(&name) else {
            let ids: Vec<&str> = guard.channels().iter().map(|c| c.id.as_str()).collect();
            send(
                &sender,
                &format!(
                    "&c[TrChat] Unknown channel '{name}'. Available: {}",
                    ids.join(", ")
                ),
            );
            return Ok(0);
        };
        // Join permission: empty permission opens the channel to everyone.
        if !channel.permission().is_empty()
            && !sender.is_console()
            && !sender.has_permission(&server, channel.permission())
        {
            send(
                &sender,
                &message("Channel-No-Join-Permission", &sender, &[&channel.id]),
            );
            return Ok(0);
        }
        let new_id = channel.id.clone();
        drop(guard);

        let mut players = SessionPlayers::global()
            .write()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(state) = players.state_mut(&me) {
            state.active_channel = new_id.clone();
            state.joined_channels.insert(new_id.to_ascii_lowercase());
        }
        drop(players);
        send(&sender, &message("Channel-Join", &sender, &[&new_id]));
        Ok(0)
    }
}

/// `/msg <target> <message>` — private chat with the configured templates.
struct MsgCommand;

impl CommandHandler for MsgCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let me = sender.get_name();
        let Some(target) = arg_string(&args, "target") else {
            send(&sender, "&cUsage: /msg <player> <message>");
            return Ok(0);
        };
        let Some(text) = arg_string(&args, "message") else {
            send(&sender, "&cUsage: /msg <player> <message>");
            return Ok(0);
        };
        if sender.is_console() {
            send(&sender, "&cConsole cannot use private messages yet.");
            return Ok(0);
        }
        // The receiver ignores the sender → the message is swallowed.
        {
            let players = SessionPlayers::global()
                .read()
                .unwrap_or_else(|e| e.into_inner());
            if players.ignores(&target, &me) {
                send(&sender, &format!("&c{target} is ignoring you."));
                return Ok(0);
            }
        }
        let online = server.get_all_players();
        let Some(target_player) = online
            .iter()
            .find(|p| p.get_name().eq_ignore_ascii_case(&target))
        else {
            send(&sender, &format!("&cPlayer {target} is not online."));
            return Ok(0);
        };

        let config = config::global_config();
        let sender_tpl = config.read().msg.sender.clone();
        let receiver_tpl = config.read().msg.receiver.clone();

        let rendered_sender = render_msg(&sender_tpl, &me, &target, &text);
        let rendered_receiver = render_msg(&receiver_tpl, &me, &target, &text);
        send(&sender, &rendered_sender);
        let _ = target_player.send_system_message(
            TextComponent::from_legacy_string_with_code(&rendered_receiver, '&'),
            false,
        );
        Ok(0)
    }
}

/// Renders a `/msg` template (`{player}`, `{target}`, `{message}`).
fn render_msg(template: &str, from: &str, to: &str, text: &str) -> String {
    template
        .replace("{player}", from)
        .replace("{target}", to)
        .replace("{message}", text)
}

/// `/trchat view <snapshot>` — §2.11 `openSnapshot`.
///
/// Opens a **read-only** 9×6 or 9×3 container holding the captured contents.
/// An unknown or expired id reports `Function-Snapshot-Expired`; the entry is
/// consumed on a successful open, matching the Mod's one-shot snapshots.
struct ViewCommand;

impl CommandHandler for ViewCommand {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let Some(id) = arg_string(&args, "snapshot") else {
            send(&sender, "&cUsage: /trchat view <snapshot>");
            return Ok(0);
        };
        let Some(player) = sender.as_player() else {
            send(&sender, "&cOnly players can open a snapshot.");
            return Ok(0);
        };
        let locale = player.get_locale();
        let Some((title, size, items)) = crate::snapshot::open(&id) else {
            // §2.11 — expired or unknown → `Function-Snapshot-Expired`.
            let text = lang::lang()
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .format("Function-Snapshot-Expired", &locale, &[]);
            send(&sender, &text);
            return Ok(0);
        };

        // §2.11 — 54 slots → `GENERIC_9x6`, otherwise `GENERIC_9x3`.
        let screen = if size == crate::functions::INVENTORY_SIZE {
            Screen::Generic9x6
        } else {
            Screen::Generic9x3
        };
        let gui = Gui::new(screen, TextComponent::from_legacy_string_with_code(&title, '&'));
        // `ReadOnlyChestMenu`: no taking, no placing.
        gui.set_allow_grab_items(false);
        gui.set_allow_put_items(false);
        // Repopulate only the live slots; padding stays empty.
        for (slot, entry) in items.iter().enumerate() {
            if let Some((registry_key, count)) = entry {
                gui.set_item(slot as u32, ItemStack::new(registry_key, *count));
            }
        }
        player.open_gui(gui);
        Ok(0)
    }
}
