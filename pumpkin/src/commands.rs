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
/// Permission for private-message spy (also granted to OPs, spec §2.6).
const PERM_SPY: &str = "trchat.spy";

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
    // `/trchat spy [on|off]` — the optional argument means the bare command
    // toggles the current state (spec §2.6, `TRC:535-542`).
    let trchat = trchat.then(
        CommandNode::literal("spy")
            .then(
                CommandNode::argument("state", &ArgumentType::String(StringType::SingleWord))
                    .execute(SpyCommand),
            )
            .execute(SpyCommand),
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

    // ---- /trreply <message> (aliases /r, /reply) ----
    let reply = Command::new(
        &[
            String::from("trreply"),
            String::from("r"),
            String::from("reply"),
        ],
        "Reply to the last player who privately messaged you",
    )
    .then(
        CommandNode::argument("message", &ArgumentType::String(StringType::Greedy))
            .execute(ReplyCommand),
    );
    context.register_command(reply, PERM_USE);

    register_bound_aliases(context);
}

/// §2.2 — registers every channel alias declared in `Bindings.Command`.
///
/// The command tree is built from config, so `/global`, `/all`, `/shout`,
/// `/staff` and the private `/msg`, `/tell`, `/w` … spellings all exist
/// without hardcoding channel names here.
///
/// The registered node takes an optional greedy `message`; an omitted body
/// makes the alias switch the active channel instead of sending.
fn register_bound_aliases(context: &Context) {
    let aliases: Vec<String> = {
        let config = config::global_config();
        let config = config.read();
        config
            .channels()
            .iter()
            .flat_map(|c| c.bindings.command.iter().cloned())
            .collect()
    };
    for alias in aliases {
        // `/msg` and friends are already registered explicitly above with a
        // dedicated handler; re-registering them would panic on the duplicate.
        if is_reserved_alias(&alias) {
            continue;
        }
        let command = Command::new(
            std::slice::from_ref(&alias),
            "Send to a channel bound to this alias",
        )
        .then(
            CommandNode::argument("message", &ArgumentType::String(StringType::Greedy))
                .execute(BoundAliasCommand {
                    alias: alias.clone(),
                }),
        );
        context.register_command(command, PERM_USE);
    }
}

/// Aliases that already have a hand-written registration earlier in
/// [`register_commands`] and must not be registered twice.
fn is_reserved_alias(alias: &str) -> bool {
    const RESERVED: &[&str] = &["msg", "tell", "r", "reply", "trreply", "channel", "trchat"];
    RESERVED.iter().any(|r| r.eq_ignore_ascii_case(alias))
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
        let Some(target) = arg_string(&args, "target") else {
            send(&sender, "&cUsage: /msg <player> <message>");
            return Ok(0);
        };
        let Some(text) = arg_string(&args, "message") else {
            send(&sender, "&cUsage: /msg <player> <message>");
            return Ok(0);
        };
        let Some(sender_player) = sender.as_player() else {
            send(&sender, &message("General-Player-Only", &sender, &[]));
            return Ok(0);
        };
        let online = server.get_all_players();
        let Some(target_player) = online
            .iter()
            .find(|p| p.get_name().eq_ignore_ascii_case(&target))
        else {
            send(&sender, &format!("&cPlayer {target} is not online."));
            return Ok(0);
        };

        // §1.6 — one shared delivery path so `/msg` and `/trreply` behave
        // identically (ignore check, rendering, spy echo).
        if !crate::private_msg::deliver(&server, &sender_player, target_player, &text) {
            send(&sender, &format!("&c{target} is ignoring you."));
        }
        Ok(0)
    }
}

/// `/global`, `/all`, `/shout`, `/staff`, … — §2.2 `executeBoundAlias`.
///
/// Every alias declared in a channel's `Bindings.Command` routes here; the
/// channel is resolved by case-insensitive lookup at dispatch time, so the
/// bundled `Global`/`Staff`/`Private` bindings work without hardcoding names.
///
/// `arguments` is the raw text after the alias: empty switches the active
/// channel (`toggleChannel`), otherwise the text is sent as a message.
struct BoundAliasCommand {
    /// The alias this node was registered under, captured at registration time
    /// because the command tree is built from config before dispatch.
    alias: String,
}

impl CommandHandler for BoundAliasCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let alias = self.alias.clone();
        let message_body = arg_string(&args, "message").unwrap_or_default();
        let config = config::global_config();
        let guard = config.read();
        let Some(channel) = guard.channel_by_command(&alias) else {
            send(
                &sender,
                &message("Channel-Command-Unbound", &sender, &[&alias]),
            );
            return Ok(0);
        };
        let channel_id = channel.id.clone();
        // §2.2 — a private channel alias with a message body is a `/msg` in
        // disguise: the first word is the target, the rest is the message.
        let is_private = channel.options.private;
        drop(guard);

        if is_private {
            let Some(sender_player) = sender.as_player() else {
                send(&sender, &message("General-Player-Only", &sender, &[]));
                return Ok(0);
            };
            if message_body.trim().is_empty() {
                // No target → treat the alias as a plain channel switch.
                return switch_channel(&sender, &sender_player.get_name(), &channel_id);
            }
            let mut parts = message_body.splitn(2, char::is_whitespace);
            let target_name = parts.next().unwrap_or_default().to_string();
            let text = parts.next().unwrap_or_default().trim().to_string();
            if text.is_empty() {
                return switch_channel(&sender, &sender_player.get_name(), &channel_id);
            }
            let online = server.get_all_players();
            let Some(target) = online
                .iter()
                .find(|p| p.get_name().eq_ignore_ascii_case(&target_name))
            else {
                send(
                    &sender,
                    &message("General-Player-Not-Found", &sender, &[&target_name]),
                );
                return Ok(0);
            };
            if !crate::private_msg::deliver(&server, &sender_player, target, &text) {
                send(&sender, &format!("&c{target_name} is ignoring you."));
            }
            return Ok(0);
        }

        // A non-private alias with no body just switches the active channel.
        if message_body.trim().is_empty() {
            return switch_channel(&sender, &sender.get_name(), &channel_id);
        }
        // With a body the alias behaves exactly like typing the channel's own
        // prefix, so the message is rewritten into that form and handed to the
        // normal chat pipeline — guards, filtering and rendering therefore stay
        // byte-identical to the prefixed spelling (§2.2).
        let Some(player) = sender.as_player() else {
            send(&sender, &message("General-Player-Only", &sender, &[]));
            return Ok(0);
        };
        let prefixed = match crate::config::global_config()
            .read()
            .channel_by_id(&channel_id)
            .and_then(|c| c.bindings.prefix.first().cloned())
        {
            Some(prefix) if !prefix.is_empty() => format!("{prefix}{message_body}"),
            // No prefix configured → the alias cannot stand in for one.
            _ => {
                send(
                    &sender,
                    &message("Channel-Command-Unbound", &sender, &[&alias]),
                );
                return Ok(0);
            }
        };
        crate::chat::dispatch_as_chat(&server, &player, &prefixed);
        Ok(0)
    }
}

/// Switches `name`'s active channel to `channel_id`, reporting `Channel-Join`.
fn switch_channel(sender: &CommandSender, name: &str, channel_id: &str) -> Result<i32, CommandError> {
    let mut players = SessionPlayers::global()
        .write()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(state) = players.state_mut(name) {
        state.active_channel = channel_id.to_string();
        state.joined_channels.insert(channel_id.to_ascii_lowercase());
    }
    drop(players);
    send(sender, &message("Channel-Join", sender, &[channel_id]));
    Ok(0)
}

/// `/trreply <message>` (aliases `/r`, `/reply`) — §2.6.
///
/// Replies to whoever last privately messaged the sender; with no recorded
/// correspondent it reports `Private-Message-No-Reply`.
struct ReplyCommand;

impl CommandHandler for ReplyCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            send(&sender, &message("General-Player-Only", &sender, &[]));
            return Ok(0);
        };
        let Some(text) = arg_string(&args, "message") else {
            send(&sender, "&cUsage: /trreply <message>");
            return Ok(0);
        };
        let me = player.get_name();
        let Some(target_name) = crate::private_msg::reply_target(&me) else {
            crate::private_msg::no_reply_hint(&player);
            return Ok(0);
        };
        // The correspondent may have gone offline since the last message.
        let online = server.get_all_players();
        let Some(target) = online
            .iter()
            .find(|p| p.get_name().eq_ignore_ascii_case(&target_name))
        else {
            send(
                &sender,
                &message("General-Player-Not-Found", &sender, &[&target_name]),
            );
            return Ok(0);
        };
        if !crate::private_msg::deliver(&server, &player, target, &text) {
            return Ok(0);
        }
        Ok(0)
    }
}

/// `/trchat spy [on|off]` — §2.6 private-message spy toggle.
///
/// Registration carries no `requires`; the check happens at runtime, where an
/// OP or the `trchat.spy` node is accepted (`TRC:535-542`).
struct SpyCommand;

impl CommandHandler for SpyCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            send(&sender, &message("General-Player-Only", &sender, &[]));
            return Ok(0);
        };
        if !player.has_permission(PERM_SPY) {
            send(&sender, &message("General-No-Permission", &sender, &[]));
            return Ok(0);
        }
        let _ = server;
        let name = player.get_name();
        // An explicit on/off wins; a bare `/trchat spy` toggles.
        let enabled = match arg_string(&args, "state") {
            Some(raw) if raw.eq_ignore_ascii_case("on") => {
                if !crate::private_msg::is_spying(&name) {
                    crate::private_msg::toggle_spy(&name);
                }
                true
            }
            Some(raw) if raw.eq_ignore_ascii_case("off") => {
                if crate::private_msg::is_spying(&name) {
                    crate::private_msg::toggle_spy(&name);
                }
                false
            }
            Some(other) => {
                send(
                    &sender,
                    &format!("&cUnknown state '{other}' (expected on/off)."),
                );
                return Ok(0);
            }
            None => crate::private_msg::toggle_spy(&name),
        };
        crate::private_msg::announce_spy(&player, enabled);
        Ok(0)
    }
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
