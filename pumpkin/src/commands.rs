//! Commands — the Bukkit v2 command surface ported to Pumpkin.
//!
//! Registered command tree (permissions mirror the upstream plugin):
//!
//! * `/trchat reload`        — re-read the config from disk (`trchat.admin`)
//! * `/trchat version`       — print the plugin version (open to everyone)
//! * `/trchat muteall`       — toggle the global chat mute (`trchat.admin`)
//! * `/trchat mute <player>` — mute a player (`trchat.admin`)
//! * `/trchat unmute <player>` — unmute a player (`trchat.admin`)
//! * `/trchat ignore <player>` — toggle ignoring a player (open to everyone)
//! * `/trchat channel join|quit …` — channel membership (open to everyone)
//! * `/trchat shadowmute <player> [on|off]` — shadow mute (§2.2)
//! * `/trchat view <snapshot>` — open a read-only inventory snapshot (§2.11)
//! * `/channel join|quit …`  — alias of `trchat channel …`
//! * `/trshadowmute`, `/shadowmute` — alias of `trchat shadowmute`
//! * `/msg <target> <msg>`   — private message (`tell` alias)
//!
//! Registration permissions are declared in [`register_permissions`]: Pumpkin
//! resolves the requirement attached by `Context::register_command` against the
//! *permission registry*, so a node that is never registered denies everyone but
//! * the console.
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
///
/// The node is always qualified with this plugin's name, because
/// `Context::register_command` prepends the plugin name to a bare node.
const PERM_USE: &str = "trchat:trchat.use";
/// Permission of administrators (reload, mute, muteall).
const PERM_ADMIN: &str = "trchat:trchat.admin";
/// Permission for private-message spy (also granted to OPs, spec §2.6).
const PERM_SPY: &str = "trchat:trchat.spy";
/// Permission to switch channels on behalf of another player (`TRC:190-204`).
const PERM_CHANNEL_OTHER: &str = "trchat:trchat.command.channel.other";
/// Permission to shadow-mute a player (spec §2.2, `TRC:886-904`).
const PERM_SHADOWMUTE: &str = "trchat:trchat.shadowmute";

/// Registers the permission nodes backing the commands above.
///
/// This is not optional book-keeping: Pumpkin resolves a *registration-time*
/// requirement through [`pumpkin_util::permission::PermissionRegistry::get_permission`],
/// and an unregistered node defaults to **deny**. Without these nodes every
/// command below would be invisible to ordinary players — only the console
/// (which is granted everything) could run them.
fn register_permissions(context: &Context) {
    use pumpkin_plugin_api::permission::{Permission, PermissionDefault, PermissionLevel};

    // Nodes mirror the upstream plugin: `/trchat status`, `/channel`, `/msg`,
    // `/ignore` and the alias commands are open to everyone; the moderation and
    // spy commands require operator level 2, matching `hasPermission(2)`
    // (`TRC:93-96`) and the `trchat.*` nodes of `PERM:44`.
    let nodes = [
        (
            PERM_USE,
            "Use TrChat chat commands",
            PermissionDefault::Allow,
        ),
        (
            PERM_ADMIN,
            "Manage TrChat (reload, mute, muteall)",
            PermissionDefault::Op(PermissionLevel::Two),
        ),
        (
            PERM_SPY,
            "Spy on private messages",
            PermissionDefault::Op(PermissionLevel::Two),
        ),
        (
            PERM_CHANNEL_OTHER,
            "Switch channels on behalf of other players",
            PermissionDefault::Op(PermissionLevel::Two),
        ),
        (
            PERM_SHADOWMUTE,
            "Shadow-mute players",
            PermissionDefault::Op(PermissionLevel::Two),
        ),
    ];

    for (node, description, default) in nodes {
        let node = Permission {
            node: node.to_string(),
            description: description.to_string(),
            default,
            children: Vec::new(),
        };
        if let Err(error) = context.register_permission(&node) {
            eprintln!("[TrChat] could not register permission: {error}");
        }
    }
}

/// The `join` / `quit` subtree shared by `/trchat channel` and `/channel`.
///
/// Built by a function because a `CommandNode` is created imperatively and both
/// entry points need their own copy.
fn channel_subtree() -> CommandNode {
    CommandNode::literal("join")
        .then(
            CommandNode::argument("channel", &ArgumentType::String(StringType::SingleWord))
                .then(
                    CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
                        .execute(ChannelJoinCommand),
                )
                .execute(ChannelJoinCommand),
        )
        .then(
            CommandNode::literal("quit")
                .then(
                    CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
                        .execute(ChannelQuitCommand),
                )
                .execute(ChannelQuitCommand),
        )
}

/// Registers every TrChat command with the given context.
///
/// Called from [`crate::TrChatPlugin::on_load`] *before* the chat pipeline is
/// initialized, so the borrow of `context` does not outlive the event handler
/// registration done by [`crate::chat::ChatManager::init`].
pub fn register_commands(context: &Context) {
    register_permissions(context);

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
        CommandNode::literal("channel")
            .then(channel_subtree())
            .execute(ChannelListCommand),
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
    // §2.2 — `/trchat shadowmute <player> [on|off]`.
    let trchat = trchat.then(
        CommandNode::literal("shadowmute")
            .then(
                CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
                    .then(
                        CommandNode::argument(
                            "state",
                            &ArgumentType::String(StringType::SingleWord),
                        )
                        .execute(ShadowMuteCommand),
                    )
                    .execute(ShadowMuteCommand),
            )
            .execute(UsageCommand),
    );
    // A root executor keeps a bare `/trchat` from answering with Pumpkin's
    // "Unknown command" error.
    let trchat = trchat.execute(UsageCommand);
    context.register_command(trchat, PERM_USE);

    // ---- /channel join|quit … ----
    let channel = Command::new(&[String::from("channel")], "Join or leave a chat channel")
        .then(channel_subtree())
        .execute(ChannelListCommand);
    context.register_command(channel, PERM_USE);

    // ---- /trshadowmute <player> [on|off] (aliases /shadowmute) ----
    // §1.3 — a standalone alias of `/trchat shadowmute`.
    let shadowmute = Command::new(
        &[String::from("trshadowmute"), String::from("shadowmute")],
        "Toggle a player's shadow mute",
    )
    .then(
        CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
            .then(
                CommandNode::argument("state", &ArgumentType::String(StringType::SingleWord))
                    .execute(ShadowMuteCommand),
            )
            .execute(ShadowMuteCommand),
    );
    context.register_command(shadowmute, PERM_SHADOWMUTE);

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

/// `/trchat channel` (bare) — lists the configured channels.
struct ChannelListCommand;

impl CommandHandler for ChannelListCommand {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let config = config::global_config();
        let ids: Vec<String> = config
            .read()
            .channels()
            .iter()
            .map(|c| c.id.clone())
            .collect();
        send(
            &sender,
            &format!(
                "&a[TrChat] Usage: /channel join|quit [channel|player]\n&aAvailable channels: {}",
                ids.join(", ")
            ),
        );
        Ok(0)
    }
}

/// `/trchat channel join <channel> [player]` / `/channel join …` — join (and
/// switch to) a channel, optionally on behalf of another player.
struct ChannelJoinCommand;

impl CommandHandler for ChannelJoinCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        // §2.4 — switching channels is a Command-Controller-managed action; with
        // the controller off (or ruleless) the sub-command is unavailable.
        let config = config::global_config();
        if !crate::command_controller::is_command_managed(config) {
            send(
                &sender,
                &message("Command-Controller-Disabled", &sender, &["channel"]),
            );
            return Ok(0);
        }

        let Some(name) = arg_string(&args, "channel") else {
            send(&sender, "&cUsage: /channel join <channel> [player]");
            return Ok(0);
        };
        // §1.2 — the optional target needs `trchat.command.channel.other`.
        let other = arg_string(&args, "player");
        let target = match &other {
            Some(target) => {
                if !sender.is_console() && !sender.has_permission(&server, PERM_CHANNEL_OTHER) {
                    send(&sender, &message("General-No-Permission", &sender, &[]));
                    return Ok(0);
                }
                match server
                    .get_all_players()
                    .iter()
                    .find(|p| p.get_name().eq_ignore_ascii_case(target))
                {
                    Some(player) => player.get_name(),
                    None => {
                        send(
                            &sender,
                            &message("General-Player-Not-Found", &sender, &[target]),
                        );
                        return Ok(0);
                    }
                }
            }
            None => {
                let Some(player) = sender.as_player() else {
                    send(&sender, &message("General-Player-Only", &sender, &[]));
                    return Ok(0);
                };
                player.get_name()
            }
        };

        let guard = config.read();
        let Some(channel) = guard.channel_by_id(&name) else {
            send(&sender, &message("Channel-Unknown", &sender, &[&name]));
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
        if let Some(state) = players.state_mut(&target) {
            state.active_channel = new_id.clone();
            state.joined_channels.insert(new_id.to_ascii_lowercase());
        }
        drop(players);

        // §1.2 — a target other than the sender reports the "other" wording.
        match other {
            Some(_) => send(
                &sender,
                &message("Channel-Join-Other", &sender, &[&target, &new_id]),
            ),
            None => send(&sender, &message("Channel-Join", &sender, &[&new_id])),
        }
        Ok(0)
    }
}

/// `/trchat channel quit [player]` / `/channel quit …` — leave the current
/// channel, falling back to the default (auto-join) channel.
struct ChannelQuitCommand;

impl CommandHandler for ChannelQuitCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let other = arg_string(&args, "player");
        let target = match &other {
            Some(target) => {
                if !sender.is_console() && !sender.has_permission(&server, PERM_CHANNEL_OTHER) {
                    send(&sender, &message("General-No-Permission", &sender, &[]));
                    return Ok(0);
                }
                match server
                    .get_all_players()
                    .iter()
                    .find(|p| p.get_name().eq_ignore_ascii_case(target))
                {
                    Some(player) => player.get_name(),
                    None => {
                        send(
                            &sender,
                            &message("General-Player-Not-Found", &sender, &[target]),
                        );
                        return Ok(0);
                    }
                }
            }
            None => {
                let Some(player) = sender.as_player() else {
                    send(&sender, &message("General-Player-Only", &sender, &[]));
                    return Ok(0);
                };
                player.get_name()
            }
        };

        let config = config::global_config();
        // The channel being left, the channel to fall back to (the `Auto-Join`
        // one, §2.4), and whether the left channel keeps its membership record
        // (`Always-Listen`, same rule as `apply_channel_toggle`).
        let (left, fallback, always_listen) = {
            let players = SessionPlayers::global()
                .read()
                .unwrap_or_else(|e| e.into_inner());
            let left = players
                .state(&target)
                .map(|state| state.active_channel.clone())
                .unwrap_or_default();
            let guard = config.read();
            let fallback = guard
                .default_channel()
                .map(|c| c.id.clone())
                .unwrap_or_else(|| "Normal".to_string());
            let always_listen = guard
                .channel_by_id(&left)
                .is_some_and(|c| c.always_listen());
            (left, fallback, always_listen)
        };

        let mut players = SessionPlayers::global()
            .write()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(state) = players.state_mut(&target) {
            if !always_listen {
                state.joined_channels.remove(&left.to_ascii_lowercase());
            }
            state.active_channel = fallback.clone();
            state.joined_channels.insert(fallback.to_ascii_lowercase());
        }
        drop(players);

        match other {
            Some(_) => send(&sender, &message("Channel-Quit-Other", &sender, &[&target])),
            None => send(&sender, &message("Channel-Quit", &sender, &[&left])),
        }
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
                // §2.2/§2.3 — a bare private alias cannot join or toggle the
                // channel, so it reports the target requirement instead.
                send(
                    &sender,
                    &message("Channel-Private-Target", &sender, &[&channel_id]),
                );
                return Ok(0);
            }
            let mut parts = message_body.splitn(2, char::is_whitespace);
            let target_name = parts.next().unwrap_or_default().to_string();
            let text = parts.next().unwrap_or_default().trim().to_string();
            if text.is_empty() {
                // A target with no body has nothing to send either.
                send(
                    &sender,
                    &message("Channel-Private-Target", &sender, &[&channel_id]),
                );
                return Ok(0);
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

        // A non-private alias with no body toggles the active channel (§2.4).
        if message_body.trim().is_empty() {
            return toggle_channel(&sender, &sender.get_name(), &channel_id);
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

/// §2.4 `toggleChannel` — joining when elsewhere, quitting when already there.
///
/// A channel with `Always-Listen` keeps its `joined` record on exit, so the
/// player stops sending to it but still receives from it.
fn toggle_channel(sender: &CommandSender, name: &str, channel_id: &str) -> Result<i32, CommandError> {
    let always_listen = {
        let config = config::global_config();
        let config = config.read();
        config
            .channel_by_id(channel_id)
            .is_some_and(|c| c.always_listen())
    };
    // `None` means the player quit; `Some(fallback)` means they joined.
    let outcome = {
        let mut players = SessionPlayers::global()
            .write()
            .unwrap_or_else(|e| e.into_inner());
        let state = players.state_mut(name);
        state.map(|s| apply_channel_toggle(s, channel_id, always_listen))
    };

    match outcome {
        Some(ChannelToggle::Quit { fallback }) => {
            send(sender, &message("Channel-Quit", sender, &[channel_id]));
            // A different active channel after quitting is announced too.
            if !fallback.eq_ignore_ascii_case(channel_id) {
                send(sender, &message("Channel-Join", sender, &[&fallback]));
            }
        }
        _ => send(sender, &message("Channel-Join", sender, &[channel_id])),
    }
    Ok(0)
}

/// The result of a [`toggle_channel`] state transition.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ChannelToggle {
    /// The player was already in the channel and left it.
    Quit { fallback: String },
    /// The player is now in the channel.
    Join,
}

/// Applies the §2.4 toggle to one player's state.
///
/// Split out from the command so the `Always-Listen` retention rule can be
/// tested without a live `CommandSender`.
fn apply_channel_toggle(
    state: &mut crate::playerdata::PlayerState,
    channel_id: &str,
    always_listen: bool,
) -> ChannelToggle {
    let lower = channel_id.to_ascii_lowercase();
    if state.joined_channels.contains(&lower) {
        // Quit — an `Always-Listen` channel keeps its membership record.
        if !always_listen {
            state.joined_channels.remove(&lower);
        }
        // Fall back to Normal when the channel being quit is the active one.
        let fallback = if state.active_channel.eq_ignore_ascii_case(channel_id)
            || state.active_channel.is_empty()
        {
            "Normal".to_string()
        } else {
            state.active_channel.clone()
        };
        state.active_channel = fallback.clone();
        return ChannelToggle::Quit { fallback };
    }
    state.active_channel = channel_id.to_string();
    state.joined_channels.insert(lower);
    ChannelToggle::Join
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

/// §2.2 — `/trchat shadowmute <player> [on|off]` (aliases `/trshadowmute`,
/// `/shadowmute`).
///
/// The Mod treats an omitted `on|off` as "toggle" (`TRC:886-904`); unlike
/// `/trchat mute` this state is per-player and only affects what the *muted*
/// player sees of their own messages (§1.3 step 6).
struct ShadowMuteCommand;

impl CommandHandler for ShadowMuteCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        // Spec: `trchat.shadowmute` (OP level 2). Registered with that node, but
        // the check is repeated here so console and OP behave identically.
        if !sender.is_console() && !sender.has_permission(&server, PERM_SHADOWMUTE) {
            send(&sender, &message("General-No-Permission", &sender, &[]));
            return Ok(0);
        }
        let Some(name) = arg_string(&args, "player") else {
            send(&sender, "&cUsage: /trchat shadowmute <player> [on|off]");
            return Ok(0);
        };
        let state = arg_string(&args, "state");

        // An unknown player has no session; report it like the other commands.
        let outcome = {
            let mut players = SessionPlayers::global()
                .write()
                .unwrap_or_else(|e| e.into_inner());
            match state.as_deref() {
                Some(raw) if raw.eq_ignore_ascii_case("on") => {
                    players.set_shadow_muted(&name, true)
                }
                Some(raw) if raw.eq_ignore_ascii_case("off") => {
                    players.set_shadow_muted(&name, false)
                }
                _ => players.toggle_shadow_muted(&name),
            }
        };

        match outcome {
            Some(true) => send(&sender, &message("Mute-Shadow-On", &sender, &[&name])),
            Some(false) => send(&sender, &message("Mute-Shadow-Off", &sender, &[&name])),
            None => {
                send(
                    &sender,
                    &message("General-Player-Not-Found", &sender, &[&name]),
                );
            }
        }
        Ok(0)
    }
}

/// Bare-command feedback: prints a one-line usage instead of letting Pumpkin
/// answer with its generic "Unknown command" error.
struct UsageCommand;

impl CommandHandler for UsageCommand {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        send(
            &sender,
            "&8[&3Tr&bChat&8] &7/trchat &fstatus&7, &freload&7, &fmute&7, &fmuteall&7, &funmute&7, &fshadowmute&7, &fspy&7, &fchannel&7, &fcolor&7, &fclear&7, &fview",
        );
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_channel_toggle, ChannelToggle};
    use crate::playerdata::PlayerState;

    /// §2.4 — joining a channel records membership and makes it active.
    #[test]
    fn toggle_joins_when_not_a_member() {
        let mut state = PlayerState {
            active_channel: "Normal".into(),
            ..Default::default()
        };

        let outcome = apply_channel_toggle(&mut state, "Global", false);

        assert_eq!(outcome, ChannelToggle::Join);
        assert_eq!(state.active_channel, "Global");
        assert!(state.joined_channels.contains("global"));
    }

    /// §2.4 — toggling a joined channel quits it and falls back to Normal.
    #[test]
    fn toggle_quits_and_falls_back_to_normal() {
        let mut state = PlayerState {
            active_channel: "Global".into(),
            ..Default::default()
        };
        state.joined_channels.insert("global".into());

        let outcome = apply_channel_toggle(&mut state, "Global", false);

        assert_eq!(
            outcome,
            ChannelToggle::Quit {
                fallback: "Normal".into()
            }
        );
        assert_eq!(state.active_channel, "Normal");
        assert!(
            !state.joined_channels.contains("global"),
            "a non Always-Listen channel drops its membership"
        );
    }

    /// §2.3 — `Always-Listen` keeps the joined record on exit, so the player
    /// still *receives* the channel even though they no longer send to it.
    #[test]
    fn always_listen_retains_membership_on_exit() {
        let mut state = PlayerState {
            active_channel: "Global".into(),
            ..Default::default()
        };
        state.joined_channels.insert("global".into());

        let outcome = apply_channel_toggle(&mut state, "Global", true);

        assert!(matches!(outcome, ChannelToggle::Quit { .. }));
        assert_eq!(state.active_channel, "Normal");
        assert!(
            state.joined_channels.contains("global"),
            "Always-Listen must retain membership on exit"
        );
    }

    /// Switching away from a *different* channel keeps that other channel
    /// active, so quitting `Global` from `Staff` does not jump to Normal.
    #[test]
    fn quit_keeps_an_unrelated_active_channel() {
        let mut state = PlayerState {
            active_channel: "Staff".into(),
            ..Default::default()
        };
        state.joined_channels.insert("global".into());

        let outcome = apply_channel_toggle(&mut state, "Global", false);

        assert_eq!(
            outcome,
            ChannelToggle::Quit {
                fallback: "Staff".into()
            }
        );
        assert_eq!(state.active_channel, "Staff");
    }

    /// Joining with a differently-cased id is idempotent on the stored key.
    #[test]
    fn toggle_membership_is_case_insensitive() {
        let mut state = PlayerState::default();
        apply_channel_toggle(&mut state, "GLOBAL", false);
        assert_eq!(state.active_channel, "GLOBAL");
        assert!(state.joined_channels.contains("global"));

        // The same channel under a different case is recognised as joined.
        let outcome = apply_channel_toggle(&mut state, "global", false);
        assert!(matches!(outcome, ChannelToggle::Quit { .. }));
        assert!(state.joined_channels.is_empty());
    }
}
