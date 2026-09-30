//! Commands — the Bukkit v2 command surface ported to Pumpkin.
//!
//! Registered command tree (permissions mirror the upstream plugin):
//!
//! * `/trchat reload`        — re-read the config from disk (`trchat.admin`)
//! * `/trchat version`       — print the plugin version (open to everyone)
//! * `/trchat status`        — plugin overview (open to everyone)
//! * `/trchat status <player>` — one player's chat state (`trchat.admin`)
//! * `/trchat mute`          — toggle the global mute (`trchat.mute`)
//! * `/trchat mute on|off`   — set the global mute explicitly (`trchat.mute`)
//! * `/trchat mute player <player> <duration> [reason]` — mute a player
//! * `/trchat unmute <player>` — clear a player's mute (`trchat.mute`)
//! * `/trchat color <color>` — set the chat colour (`trchat.command.color`)
//! * `/trmute`, `/mute`, `/trunmute` — standalone aliases of the above
//! * `/trchat ignore <player> [on|off]` — toggle ignoring a player (open)
//! * `/ignore`, `/trignore <player> [on|off]`, `/ignorelist` — §1.3 aliases
//! * `/trspy [on|off]`       — standalone alias of `/trchat spy`
//! * `/trchat channel join|quit …` — channel membership (open to everyone)
//! * `/trchat shadowmute <player> [on|off]` — shadow mute (§2.2)
//! * `/trchat view <snapshot>` — open a read-only inventory snapshot (§2.11)
//! * `/channel join|quit …`  — alias of `trchat channel …`
//! * `/trshadowmute`, `/shadowmute` — alias of `trchat shadowmute`
//! * `/msg <target> <msg>`   — private message (`tell`, `/trmsg` aliases)
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
        Arg, ArgumentType, Command, CommandError, CommandNode, CommandSender, CommandSuggestion,
        CommandSuggestions, ConsumedArgs, StringType, SuggestionRequest,
    },
    commands::{CommandHandler, CommandSuggestionHandler},
    gui::Gui,
    text::TextComponent,
    Context, ItemStack, Screen, Server,
};

use crate::condition;
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
/// Permission to mute players and toggle the global mute (spec §2.2).
const PERM_MUTE: &str = "trchat:trchat.mute";
/// Permission to ignore players — open to everyone (`PERM:46-48`).
const PERM_IGNORE: &str = "trchat:trchat.command.ignore";
/// Permission to set one's own chat colour (OP level 2, `PERM:49`).
const PERM_COLOR: &str = "trchat:trchat.command.color";
/// The 16 `trchat.color.<code>` nodes that gate using a chat colour
/// (`PERM:56-69`). All default to OP level 2.
const COLOR_CODES: &str = "0123456789abcdef";

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
        (
            PERM_MUTE,
            "Mute players and toggle the global mute",
            PermissionDefault::Op(PermissionLevel::Two),
        ),
        (
            PERM_IGNORE,
            "Ignore other players",
            // `trchat.command.ignore` is one of the always-open nodes
            // (`PERM:131-135`).
            PermissionDefault::Allow,
        ),
        (
            PERM_COLOR,
            "Set your own chat colour",
            PermissionDefault::Op(PermissionLevel::Two),
        ),
    ];
    for node in &nodes {
        let permission = Permission {
            node: node.0.to_string(),
            description: node.1.to_string(),
            default: node.2,
            children: Vec::new(),
        };
        if let Err(error) = context.register_permission(&permission) {
            eprintln!("[TrChat] could not register permission {}: {error}", node.0);
        }
    }
    // §2.1 — the 16 `trchat.color.<code>` nodes, one per hex digit.
    for code in COLOR_CODES.chars() {
        let permission = Permission {
            node: format!("trchat:trchat.color.{code}"),
            description: format!("Use &{code} as a chat colour"),
            default: PermissionDefault::Op(PermissionLevel::Two),
            children: Vec::new(),
        };
        if let Err(error) = context.register_permission(&permission) {
            eprintln!("[TrChat] could not register permission trchat.color.{code}: {error}");
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
                .suggest(ChannelIds)
                .then(
                    CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
                        .suggest(PlayerNames)
                        .execute(ChannelJoinCommand),
                )
                .execute(ChannelJoinCommand),
        )
        .then(
            CommandNode::literal("quit")
                .then(
                    CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
                        .suggest(PlayerNames)
                        .execute(ChannelQuitCommand),
                )
                .execute(ChannelQuitCommand),
        )
}

/// §1.6 — tab-completes online player names.
struct PlayerNames;

impl CommandSuggestionHandler for PlayerNames {
    fn suggest(
        &self,
        _sender: CommandSender,
        server: Server,
        request: SuggestionRequest,
    ) -> CommandSuggestions {
        suggest_matching(
            &request,
            server.get_all_players().iter().map(|p| p.get_name()),
        )
    }
}

/// §1.6 — tab-completes the id of every joinable channel, that is every channel
/// that is not `Options.Private` (`chat.md:135`, `TRC:178-183`).
struct ChannelIds;

impl CommandSuggestionHandler for ChannelIds {
    fn suggest(
        &self,
        _sender: CommandSender,
        _server: Server,
        request: SuggestionRequest,
    ) -> CommandSuggestions {
        let config = config::global_config();
        let ids = {
            let guard = config.read();
            guard
                .channels()
                .iter()
                .filter(|channel| !channel.options.private)
                .map(|channel| channel.id.clone())
                .collect::<Vec<_>>()
        };
        suggest_matching(&request, ids.into_iter())
    }
}

/// §1.6 — tab-completes the fixed duration literals the upstream offers
/// (`TRC:117-120`). Free-form durations such as `1h30m` still parse; these are
/// only the suggestions.
struct Durations;

/// The duration literals shown by `/trchat mute player <player> <duration>`.
const DURATION_LITERALS: [&str; 6] = ["30s", "5m", "1h", "1d", "7d", "permanent"];

impl CommandSuggestionHandler for Durations {
    fn suggest(
        &self,
        _sender: CommandSender,
        _server: Server,
        request: SuggestionRequest,
    ) -> CommandSuggestions {
        suggest_matching(
            &request,
            DURATION_LITERALS.iter().map(|item| (*item).to_string()),
        )
    }
}

/// §1.6 — tab-completes the chat colours the sender may use, plus `reset`.
///
/// Without a player (console) only `reset` is offered, matching
/// `TRC:210-214, 687-694`.
struct Colors;

impl CommandSuggestionHandler for Colors {
    fn suggest(
        &self,
        sender: CommandSender,
        _server: Server,
        request: SuggestionRequest,
    ) -> CommandSuggestions {
        let mut candidates: Vec<String> = Vec::new();
        if let Some(player) = sender.as_player() {
            for code in COLOR_CODES.chars() {
                if player.has_permission(&format!("trchat.color.{code}")) {
                    candidates.push(code.to_string());
                }
            }
        }
        candidates.push("reset".to_string());
        suggest_matching(&request, candidates.into_iter())
    }
}

/// Case-insensitive prefix filter shared by the suggestion handlers above.
fn suggest_matching(
    request: &SuggestionRequest,
    candidates: impl Iterator<Item = String>,
) -> CommandSuggestions {
    let prefix = request.remaining.to_ascii_lowercase();
    CommandSuggestions {
        start: request.start,
        // The whole current token is replaced, as the WIT request describes.
        length: request.remaining.len() as u32,
        values: candidates
            .filter(|candidate| candidate.to_ascii_lowercase().starts_with(&prefix))
            .map(|value| CommandSuggestion {
                value,
                tooltip: None,
            })
            .collect(),
    }
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
    // §1.2 — `/trchat status` is open to everyone; `status <player>` needs
    // `trchat.admin`, checked at runtime because the guest API has no
    // per-subcommand requirement.
    .then(
        CommandNode::literal("status")
            .then(
                CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
                    .suggest(PlayerNames)
                    .execute(PlayerStatusCommand),
            )
            .execute(StatusCommand),
    )
    // §1.2 — `/trchat mute` toggles the global mute, `mute on|off` sets it, and
    // `mute player <player> <duration> [reason]` mutes one player.
    .then(
        CommandNode::literal("mute")
            .then(CommandNode::literal("on").execute(MuteStateCommand { muted: true }))
            .then(CommandNode::literal("off").execute(MuteStateCommand { muted: false }))
            .then(
                CommandNode::literal("player").then(
                    CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
                        .suggest(PlayerNames)
                        .then(
                            CommandNode::argument(
                                "duration",
                                &ArgumentType::String(StringType::SingleWord),
                            )
                            .suggest(Durations)
                            .then(
                                CommandNode::argument(
                                    "reason",
                                    &ArgumentType::String(StringType::Greedy),
                                )
                                .execute(MuteCommand),
                            )
                            .execute(MuteCommand),
                        ),
                ),
            )
            .execute(MuteAllCommand),
    )
    .then(
        CommandNode::literal("unmute").then(
            CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
                .suggest(PlayerNames)
                .execute(UnmuteCommand),
        ),
    )
    // §1.2 — `/trchat color <color>` sets the sender's chat colour.
    .then(
        CommandNode::literal("color").then(
            CommandNode::argument("color", &ArgumentType::String(StringType::SingleWord))
                .suggest(Colors)
                .execute(ColorCommand),
        ),
    )
    .then(
        CommandNode::literal("ignore").then(
            CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
                .suggest(PlayerNames)
                .then(
                    CommandNode::argument("state", &ArgumentType::String(StringType::SingleWord))
                        .execute(IgnoreCommand),
                )
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
            .suggest(PlayerNames)
            .then(
                CommandNode::argument("state", &ArgumentType::String(StringType::SingleWord))
                    .execute(ShadowMuteCommand),
            )
            .execute(ShadowMuteCommand),
    );
    context.register_command(shadowmute, PERM_SHADOWMUTE);

    // ---- /trmute, /mute, /trunmute ----
    // §1.3 — the standalone mute commands are the player-mute form only: no
    // `on|off` branch (`TRC:860-884`), and they share one command object.
    let mute = Command::new(
        &[String::from("trmute"), String::from("mute")],
        "Mute a player",
    )
    .then(
        CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
            .suggest(PlayerNames)
            .then(
                CommandNode::argument("duration", &ArgumentType::String(StringType::SingleWord))
                    .suggest(Durations)
                    .then(
                        CommandNode::argument("reason", &ArgumentType::String(StringType::Greedy))
                            .execute(MuteCommand),
                    )
                    .execute(MuteCommand),
            ),
    );
    context.register_command(mute, PERM_MUTE);

    let unmute = Command::new(&[String::from("trunmute")], "Unmute a player").then(
        CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
            .suggest(PlayerNames)
            .execute(UnmuteCommand),
    );
    context.register_command(unmute, PERM_MUTE);

    // ---- /msg <target> <message> (aliases /tell, /trmsg) ----
    let msg = Command::new(
        &[
            String::from("msg"),
            String::from("tell"),
            String::from("trmsg"),
        ],
        "Send a private message to a player",
    )
    .then(
        CommandNode::argument("target", &ArgumentType::String(StringType::SingleWord)).then(
            CommandNode::argument("message", &ArgumentType::String(StringType::Greedy))
                .execute(MsgCommand),
        ),
    );
    context.register_command(msg, PERM_USE);

    // ---- /trspy [on|off] (standalone alias of `/trchat spy`) ----
    let spy = Command::new(&[String::from("trspy")], "Toggle private-message spy")
        .then(
            CommandNode::argument("state", &ArgumentType::String(StringType::SingleWord))
                .execute(SpyCommand),
        )
        .execute(SpyCommand);
    context.register_command(spy, PERM_USE);

    // ---- /ignore, /trignore <player> [on|off] and /ignorelist ----
    // §1.3 — the standalone spellings share the `/trchat ignore` executor.
    let ignore = Command::new(
        &[String::from("ignore"), String::from("trignore")],
        "Ignore or unignore a player",
    )
    .then(
        CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
            .suggest(PlayerNames)
            .then(
                CommandNode::argument("state", &ArgumentType::String(StringType::SingleWord))
                    .execute(IgnoreCommand),
            )
            .execute(IgnoreCommand),
    );
    context.register_command(ignore, PERM_IGNORE);

    let ignorelist = Command::new(&[String::from("ignorelist")], "List ignored players")
        .execute(IgnoreListCommand);
    context.register_command(ignorelist, PERM_IGNORE);

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
    const RESERVED: &[&str] = &[
        "msg",
        "tell",
        "trmsg",
        "r",
        "reply",
        "trreply",
        "channel",
        "trchat",
        "ignore",
        "trignore",
        "ignorelist",
        "trspy",
        "trmute",
        "mute",
        "trunmute",
        "trshadowmute",
        "shadowmute",
    ];
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

/// `/trchat status` — plugin overview, open to everyone (spec §1.2).
struct StatusCommand;

impl CommandHandler for StatusCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        send(&sender, &status_overview(&sender, &server));
        Ok(0)
    }
}

/// `/trchat status <player>` — one player's chat state (`trchat.admin`).
struct PlayerStatusCommand;

impl CommandHandler for PlayerStatusCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        if !sender.has_permission(&server, PERM_ADMIN) {
            send(&sender, &message("General-No-Permission", &sender, &[]));
            return Ok(0);
        }
        let Some(name) = arg_string(&args, "player") else {
            send(&sender, "&cUsage: /trchat status <player>");
            return Ok(0);
        };
        let Some(target) = server.get_player_by_name(&name) else {
            send(
                &sender,
                &message("General-Player-Not-Found", &sender, &[&name]),
            );
            return Ok(0);
        };
        send(&sender, &player_status_report(&sender, &target));
        Ok(0)
    }
}

/// `Status-State-Enabled` / `Status-State-Disabled` for a boolean flag.
fn state_text(sender: &CommandSender, enabled: bool) -> String {
    let key = if enabled {
        "Status-State-Enabled"
    } else {
        "Status-State-Disabled"
    };
    message(key, sender, &[])
}

/// The `/trchat status` block: the overview line plus the creator, original
/// author and repository credits closed by the footer.
///
/// Deviations, both forced by the runtime: Redis has no implementation in this
/// port, so its state is reported as *disabled* rather than connected; and the
/// link/credit lines are appended as plain text instead of click actions
/// because the WIT feedback channel carries a single component per line.
fn status_overview(sender: &CommandSender, server: &Server) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let controller = {
        let config = config::global_config();
        let config = config.read();
        // The default channel is the `Auto-Join` one, else the first channel.
        let default_channel = config
            .default_channel()
            .map(|channel| channel.id.clone())
            .unwrap_or_else(|| "-".to_string());
        let channel_count = config.channels().len().to_string();
        let controller = (
            config.function.command_controller.enabled,
            config.function.command_controller.rules.len().to_string(),
        );
        (channel_count, default_channel, controller)
    };
    let (channel_count, default_channel, (controller_enabled, rule_count)) = controller;

    let globally_muted = {
        let players = SessionPlayers::global()
            .read()
            .unwrap_or_else(|e| e.into_inner());
        players.is_global_muted()
    };

    let overview = message(
        "Status-Overview",
        sender,
        &[
            version,
            &channel_count,
            &default_channel,
            // Redis is not implemented by this port.
            &message("Status-State-Disabled", sender, &[]),
            &state_text(sender, globally_muted),
            &state_text(sender, controller_enabled),
            &rule_count,
            &server.get_player_count().to_string(),
            &server.get_max_players().to_string(),
        ],
    );
    let credits = format!(
        "{}\n{}\n{}{}\n{}",
        message("Status-Creator-Prefix", sender, &[])
            + &message("Status-Creator-Link", sender, &[]),
        message("Status-Original-Author", sender, &[]),
        message("Status-Repository-Prefix", sender, &[]),
        message("Status-Repository-Link", sender, &[]),
        message("Status-Footer", sender, &[]),
    );
    format!("{overview}\n{credits}")
}

/// The `/trchat status <player>` block (spec §1.2).
///
/// A player who is online but has no session state yet (the join event has not
/// been observed) reports the configured default channel and zero joined
/// channels rather than failing.
fn player_status_report(
    sender: &CommandSender,
    target: &pumpkin_plugin_api::player::Player,
) -> String {
    let name = target.get_name();
    let (channel, joined, shadow, spy, muted, mute_until, mute_reason) = {
        let session = SessionPlayers::global();
        let session = session.read().unwrap_or_else(|e| e.into_inner());
        let state = session.state(&name);
        (
            state.map(|state| state.active_channel.clone()),
            state.map_or(0, |state| state.joined_channels.len()),
            state.is_some_and(|state| state.shadow_muted),
            state.is_some_and(|state| state.private_spy),
            session.is_muted(&name),
            session.mute_state(&name).map(|(until, _)| until),
            session
                .mute_state(&name)
                .map(|(_, reason)| reason.to_string()),
        )
    };
    let channel = channel.unwrap_or_else(|| {
        let config = config::global_config();
        let config = config.read();
        config
            .default_channel()
            .map(|channel| channel.id.clone())
            .unwrap_or_else(|| "-".to_string())
    });

    let mut report = message(
        "Player-Status-Overview",
        sender,
        &[
            &name,
            &channel,
            &joined.to_string(),
            &target.get_ping().to_string(),
            &state_text(sender, muted),
            &state_text(sender, shadow),
            &state_text(sender, spy),
            &state_text(sender, condition::is_op(target)),
            game_mode_name(target.get_gamemode()),
        ],
    );
    if muted {
        if let Some(until) = mute_until {
            let expiry = if until < 0 {
                message("Player-Status-Permanent", sender, &[])
            } else {
                crate::playerdata::mute_expiry_text(until)
            };
            let reason = {
                let reason = mute_reason.unwrap_or_default();
                if reason.is_empty() {
                    "-".to_string()
                } else {
                    reason
                }
            };
            report = format!(
                "{report}\n{}",
                message("Player-Status-Mute-Detail", sender, &[&expiry, &reason])
            );
        }
    }
    format!("{report}\n{}", message("Status-Footer", sender, &[]))
}

/// The game mode shown by `Player-Status-Overview` `{8}`.
fn game_mode_name(mode: pumpkin_plugin_api::common::GameMode) -> &'static str {
    use pumpkin_plugin_api::common::GameMode;
    match mode {
        GameMode::Survival => "survival",
        GameMode::Creative => "creative",
        GameMode::Adventure => "adventure",
        GameMode::Spectator => "spectator",
    }
}

/// What `/trchat color <color>` asked for, after the upstream normalisation
/// (`ChatService.setChatColor`): trim, lowercase (`ROOT`), then match `[0-9a-f]`.
#[derive(Debug, PartialEq, Eq)]
enum ColorRequest {
    /// `reset` / `null` / `default` — clear the stored colour.
    Reset,
    /// A valid one-character hex colour code.
    Set(char),
    /// Anything else; the *original* argument is echoed by `Color-Invalid`.
    Invalid,
}

fn parse_color_request(raw: &str) -> ColorRequest {
    let value = raw.trim().to_ascii_lowercase();
    if matches!(value.as_str(), "reset" | "null" | "default") {
        return ColorRequest::Reset;
    }
    let mut chars = value.chars();
    match (chars.next(), chars.next()) {
        // Lowercased already, so an ASCII hex digit is exactly `[0-9a-f]`.
        (Some(code), None) if code.is_ascii_hexdigit() => ColorRequest::Set(code),
        _ => ColorRequest::Invalid,
    }
}

/// Stores (or clears) `name`'s chat colour in the session store.
fn store_chat_color(name: &str, colour: Option<char>) {
    SessionPlayers::global()
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .set_chat_color(name, colour);
}

/// `/trchat color <color>` — set/reset the sender's chat colour.
struct ColorCommand;

impl CommandHandler for ColorCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        if !sender.has_permission(&server, PERM_COLOR) {
            send(&sender, &message("General-No-Permission", &sender, &[]));
            return Ok(0);
        }
        let Some(player) = sender.as_player() else {
            send(&sender, &message("General-Player-Only", &sender, &[]));
            return Ok(0);
        };
        let Some(raw) = arg_string(&args, "color") else {
            send(&sender, "&cUsage: /trchat color <color>");
            return Ok(0);
        };
        let code = match parse_color_request(&raw) {
            ColorRequest::Reset => {
                store_chat_color(&player.get_name(), None);
                send(&sender, &message("Color-Reset", &sender, &[]));
                return Ok(0);
            }
            ColorRequest::Invalid => {
                send(&sender, &message("Color-Invalid", &sender, &[&raw]));
                return Ok(0);
            }
            ColorRequest::Set(code) => code,
        };
        // Operators may use any colour; otherwise the matching
        // `trchat.color.<code>` node is required (`ChatService.java:315-322`).
        if !condition::is_op(&player) && !player.has_permission(&format!("trchat.color.{code}")) {
            send(&sender, &message("General-No-Permission", &sender, &[]));
            return Ok(0);
        }
        store_chat_color(&player.get_name(), Some(code));
        // The sample is the colour code applied to its own letter, exactly as
        // the upstream passes `"&" + color + color`.
        let sample = format!("&{code}{code}");
        send(&sender, &message("Color-Selected", &sender, &[&sample]));
        Ok(0)
    }
}

/// §6 `parseDuration` — `s/m/h/d/w` segments plus the permanent keywords.
///
/// The whole string must be consumed and the total must be positive, so `5x`,
/// `1h30` and `0s` are all invalid. Returns `None` for a malformed input and
/// `Some(-1)` for the permanent spellings, matching the upstream contract where
/// a negative duration marks a permanent mute (`ModerationService.java:148-178`).
fn parse_duration(raw: &str) -> Option<i64> {
    let text = raw.trim();
    if text.is_empty() {
        return None;
    }
    let lowered = text.to_ascii_lowercase();
    if matches!(lowered.as_str(), "permanent" | "forever" | "perm" | "永久") {
        return Some(-1);
    }

    let mut total: i64 = 0;
    let mut number = String::new();
    for ch in lowered.chars() {
        if ch.is_ascii_digit() {
            number.push(ch);
            continue;
        }
        // A unit must directly follow at least one digit.
        let unit = match ch {
            's' => 1_000,
            'm' => 60_000,
            'h' => 3_600_000,
            'd' => 86_400_000,
            'w' => 604_800_000,
            _ => return None,
        };
        let value: i64 = number.parse().ok()?;
        number.clear();
        // `Math.addExact`/`multiplyExact` in the upstream; `checked_*` here.
        total = total.checked_add(value.checked_mul(unit)?)?;
    }
    // A trailing number with no unit means the string was not fully consumed.
    if !number.is_empty() {
        return None;
    }
    (total > 0).then_some(total)
}

/// §6 `muteExpiry` — see [`crate::playerdata::mute_expiry_text`].
fn mute_expiry_text(until: i64) -> String {
    crate::playerdata::mute_expiry_text(until)
}

/// `/trchat mute` (bare) — toggle the global mute.
struct MuteAllCommand;

impl CommandHandler for MuteAllCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        if !sender.has_permission(&server, PERM_MUTE) {
            send(&sender, &message("General-No-Permission", &sender, &[]));
            return Ok(0);
        }
        let current = {
            let players = SessionPlayers::global()
                .read()
                .unwrap_or_else(|e| e.into_inner());
            players.is_global_muted()
        };
        set_global_mute(&sender, &server, !current);
        Ok(0)
    }
}

/// `/trchat mute on|off` — set the global mute explicitly (spec §1.2).
///
/// `on` and `off` are literal nodes, so the target state travels in the handler
/// rather than through an argument.
struct MuteStateCommand {
    /// The state this literal requests.
    muted: bool,
}

impl CommandHandler for MuteStateCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        if !sender.has_permission(&server, PERM_MUTE) {
            send(&sender, &message("General-No-Permission", &sender, &[]));
            return Ok(0);
        }
        set_global_mute(&sender, &server, self.muted);
        Ok(0)
    }
}

/// Applies the global mute and announces it to every online player, as
/// `ChatService.setGlobalMute` does (`ChatService.java:332-342`).
fn set_global_mute(sender: &CommandSender, server: &Server, muted: bool) {
    {
        let mut players = SessionPlayers::global()
            .write()
            .unwrap_or_else(|e| e.into_inner());
        players.set_global_muted(muted);
    }
    // The announcement goes to everyone online, including the issuer.
    let key = if muted {
        "Global-Mute-On"
    } else {
        "Global-Mute-Off"
    };
    let text = message(key, sender, &[]);
    for online in server.get_all_players() {
        online.send_system_message(
            TextComponent::from_legacy_string_with_code(&text, '&'),
            false,
        );
    }
}

/// `/trchat mute player <player> <duration> [reason]` — mute one player.
struct MuteCommand;

impl CommandHandler for MuteCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        if !sender.has_permission(&server, PERM_MUTE) {
            send(&sender, &message("General-No-Permission", &sender, &[]));
            return Ok(0);
        }
        let Some(name) = arg_string(&args, "player") else {
            send(
                &sender,
                "&cUsage: /trchat mute player <player> <duration> [reason]",
            );
            return Ok(0);
        };
        let Some(raw_duration) = arg_string(&args, "duration") else {
            send(
                &sender,
                "&cUsage: /trchat mute player <player> <duration> [reason]",
            );
            return Ok(0);
        };
        let Some(duration) = parse_duration(&raw_duration) else {
            send(
                &sender,
                &message("Mute-Wrong-Format", &sender, &[&raw_duration]),
            );
            return Ok(0);
        };
        if !player_exists(&server, &name) {
            send(
                &sender,
                &message("General-Player-Not-Found", &sender, &[&name]),
            );
            return Ok(0);
        }
        // An omitted reason is reported as `-` by the store.
        let reason = arg_string(&args, "reason").unwrap_or_default();

        let applied = {
            let mut players = SessionPlayers::global()
                .write()
                .unwrap_or_else(|e| e.into_inner());
            players.mute(&name, duration, &reason)
        };
        match applied {
            Some(until) => send(
                &sender,
                &message(
                    "Mute-Muted-Player",
                    &sender,
                    &[&name, &mute_expiry_text(until), &reason_or_dash(&reason)],
                ),
            ),
            None => send(
                &sender,
                &message("General-Player-Not-Found", &sender, &[&name]),
            ),
        }
        Ok(0)
    }
}

/// The `-` the store substitutes for a blank mute reason.
fn reason_or_dash(reason: &str) -> String {
    if reason.trim().is_empty() {
        "-".to_string()
    } else {
        reason.trim().to_string()
    }
}

/// Whether `name` belongs to an online player (case-insensitive).
///
/// The upstream ignore command also consults the Redis-known player list;
/// without Redis this port only knows who is online.
fn player_exists(server: &Server, name: &str) -> bool {
    server
        .get_all_players()
        .iter()
        .any(|player| player.get_name().eq_ignore_ascii_case(name))
}

/// `/trchat unmute <player>` — clear a player's mute.
struct UnmuteCommand;

impl CommandHandler for UnmuteCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        if !sender.has_permission(&server, PERM_MUTE) {
            send(&sender, &message("General-No-Permission", &sender, &[]));
            return Ok(0);
        }
        let Some(name) = arg_string(&args, "player") else {
            send(&sender, "&cUsage: /trchat unmute <player>");
            return Ok(0);
        };
        if !player_exists(&server, &name) {
            send(
                &sender,
                &message("General-Player-Not-Found", &sender, &[&name]),
            );
            return Ok(0);
        }
        let cleared = {
            let mut players = SessionPlayers::global()
                .write()
                .unwrap_or_else(|e| e.into_inner());
            players.unmute(&name)
        };
        match cleared {
            Some(_) => send(
                &sender,
                &message("Mute-Cancel-Muted-Player", &sender, &[&name]),
            ),
            None => send(
                &sender,
                &message("General-Player-Not-Found", &sender, &[&name]),
            ),
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
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        if !sender.has_permission(&server, PERM_IGNORE) {
            send(&sender, &message("General-No-Permission", &sender, &[]));
            return Ok(0);
        }
        let me = sender.get_name();
        let Some(target) = arg_string(&args, "player") else {
            send(&sender, "&cUsage: /ignore <player> [on|off]");
            return Ok(0);
        };
        if target.eq_ignore_ascii_case(&me) {
            send(&sender, &message("Ignore-Self", &sender, &[]));
            return Ok(0);
        }
        // The target has to be a known player. The upstream also consults the
        // Redis-known list; this port only knows who is online.
        if !player_exists(&server, &target) {
            send(
                &sender,
                &message("General-Player-Not-Found", &sender, &[&target]),
            );
            return Ok(0);
        }
        // `on` / `off` are explicit; an omitted state toggles.
        let requested = match arg_string(&args, "state") {
            Some(raw) if raw.eq_ignore_ascii_case("on") => Some(true),
            Some(raw) if raw.eq_ignore_ascii_case("off") => Some(false),
            Some(other) => {
                send(
                    &sender,
                    &format!("&cUnknown state '{other}' (expected on/off)."),
                );
                return Ok(0);
            }
            None => None,
        };
        let outcome = {
            let mut players = SessionPlayers::global()
                .write()
                .unwrap_or_else(|e| e.into_inner());
            players
                .state_mut(&me)
                .map(|state| apply_ignore(&mut state.ignored, &target, requested))
        };
        match outcome {
            Some(now_ignored) => {
                let key = if now_ignored {
                    "Ignore-Ignored-Player"
                } else {
                    "Ignore-Cancel-Player"
                };
                send(&sender, &message(key, &sender, &[&target]));
            }
            // No session state means the join event has not been seen yet.
            None => send(&sender, "&c[TrChat] Your chat session is not ready yet."),
        }
        Ok(0)
    }
}

/// `/ignorelist` — the players the sender ignores (`TRC:805-808`).
struct IgnoreListCommand;

impl CommandHandler for IgnoreListCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        if !sender.has_permission(&server, PERM_IGNORE) {
            send(&sender, &message("General-No-Permission", &sender, &[]));
            return Ok(0);
        }
        let me = sender.get_name();
        let list = {
            let players = SessionPlayers::global()
                .read()
                .unwrap_or_else(|e| e.into_inner());
            players.state(&me).map(|state| {
                let mut names: Vec<&str> = state.ignored.iter().map(String::as_str).collect();
                names.sort_unstable();
                names.join(", ")
            })
        };
        // An empty list renders as the literal `-` (spec §1.3).
        let list = list
            .filter(|list| !list.is_empty())
            .unwrap_or_else(|| "-".to_string());
        send(&sender, &message("Ignore-List", &sender, &[&list]));
        Ok(0)
    }
}

/// Applies an `/ignore` request to `ignored` and reports whether `target` ends
/// up ignored.
///
/// `requested` is `Some(true)` for `on`, `Some(false)` for `off` and `None` for
/// the toggle form. Names are stored lowercased, like the rest of the store.
fn apply_ignore(
    ignored: &mut std::collections::HashSet<String>,
    target: &str,
    requested: Option<bool>,
) -> bool {
    let lower = target.to_ascii_lowercase();
    let now_ignored = requested.unwrap_or_else(|| !ignored.contains(&lower));
    if now_ignored {
        ignored.insert(lower);
    } else {
        ignored.remove(&lower);
    }
    now_ignored
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
fn toggle_channel(
    sender: &CommandSender,
    name: &str,
    channel_id: &str,
) -> Result<i32, CommandError> {
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
    use super::{apply_channel_toggle, parse_duration, suggest_matching, ChannelToggle};
    use crate::playerdata::PlayerState;
    use pumpkin_plugin_api::command::SuggestionRequest;

    /// §6 `parseDuration` — single and combined `s/m/h/d/w` segments, plus the
    /// permanent spellings that map onto the `-1` marker.
    #[test]
    fn durations_parse_segments_and_permanent_keywords() {
        assert_eq!(parse_duration("30s"), Some(30_000));
        assert_eq!(parse_duration("5m"), Some(300_000));
        assert_eq!(parse_duration("1h"), Some(3_600_000));
        assert_eq!(parse_duration("1d"), Some(86_400_000));
        assert_eq!(parse_duration("7d"), Some(604_800_000));
        assert_eq!(parse_duration("1w"), Some(604_800_000));
        // Segments are summed, and the input may be padded.
        assert_eq!(parse_duration(" 1h30m "), Some(5_400_000));
        assert_eq!(
            parse_duration("1M"),
            Some(60_000),
            "units are case-insensitive"
        );
        // Only the *total* must be positive, so a zero-valued trailing segment
        // is still a fully consumed, valid duration.
        assert_eq!(parse_duration("1h0m"), Some(3_600_000));

        for permanent in ["permanent", "PERMANENT", "forever", "perm", "永久"] {
            assert_eq!(parse_duration(permanent), Some(-1), "{permanent}");
        }
    }

    /// The whole string must be consumed and the total must be positive.
    #[test]
    fn malformed_durations_are_rejected() {
        for invalid in [
            "", "   ", "5", "5x", "1h30", "abc", "s", "-5s", "`5s`", "0s", "0m",
        ] {
            assert_eq!(parse_duration(invalid), None, "{invalid:?} must not parse");
        }
    }

    /// Overflow is rejected rather than wrapping, mirroring `Math.multiplyExact`.
    #[test]
    fn overflowing_durations_are_rejected() {
        assert_eq!(parse_duration("9999999999999999w"), None);
    }

    /// `Player-Status-Overview` `{8}` reports the lowercase game mode name.
    #[test]
    fn game_modes_render_in_lowercase() {
        use pumpkin_plugin_api::common::GameMode;
        assert_eq!(super::game_mode_name(GameMode::Survival), "survival");
        assert_eq!(super::game_mode_name(GameMode::Creative), "creative");
        assert_eq!(super::game_mode_name(GameMode::Adventure), "adventure");
        assert_eq!(super::game_mode_name(GameMode::Spectator), "spectator");
    }

    /// `ChatService.setChatColor` — trim + lowercase, then reset keywords, then
    /// a single `[0-9a-f]` code; everything else is invalid.
    #[test]
    fn color_requests_follow_the_upstream_normalisation() {
        use super::ColorRequest::{Invalid, Reset, Set};
        assert_eq!(super::parse_color_request("reset"), Reset);
        assert_eq!(super::parse_color_request(" NULL "), Reset);
        assert_eq!(super::parse_color_request("Default"), Reset);

        assert_eq!(super::parse_color_request("a"), Set('a'));
        assert_eq!(super::parse_color_request(" A "), Set('a'), "lowercased");
        assert_eq!(super::parse_color_request("f"), Set('f'));

        // `&a` is *not* stripped by the upstream, and only one char is allowed.
        for bad in ["", "&a", "§a", "red", "ab", "g", "1 2", "aa"] {
            assert_eq!(super::parse_color_request(bad), Invalid, "{bad:?}");
        }
    }

    /// §1.3 — `on` / `off` are explicit, an omitted state toggles, and names are
    /// compared case-insensitively.
    #[test]
    fn ignore_requests_set_clear_and_toggle() {
        use std::collections::HashSet;
        let mut ignored = HashSet::new();

        assert!(super::apply_ignore(&mut ignored, "Bob", None), "toggles on");
        assert_eq!(ignored.len(), 1);
        assert!(ignored.contains("bob"), "stored lowercased");

        // Toggling again clears it.
        assert!(!super::apply_ignore(&mut ignored, "bob", None));
        assert!(ignored.is_empty());

        // Explicit states are idempotent.
        assert!(super::apply_ignore(&mut ignored, "Carol", Some(true)));
        assert!(super::apply_ignore(&mut ignored, "CAROL", Some(true)));
        assert_eq!(ignored.len(), 1, "re-ignoring is not a duplicate");
        assert!(!super::apply_ignore(&mut ignored, "carol", Some(false)));
        // Unignoring again keeps it unignored.
        assert!(!super::apply_ignore(&mut ignored, "carol", Some(false)));
        assert!(ignored.is_empty(), "unignoring twice is still unignored");
    }

    fn request(remaining: &str) -> SuggestionRequest {
        SuggestionRequest {
            input: format!("/trchat channel join {remaining}"),
            cursor: 22,
            start: 22,
            remaining: remaining.to_string(),
        }
    }

    fn suggest(remaining: &str, candidates: &[&str]) -> Vec<String> {
        suggest_matching(
            &request(remaining),
            candidates.iter().map(|c| c.to_string()),
        )
        .values
        .into_iter()
        .map(|s| s.value)
        .collect()
    }

    /// §1.6 — completions are a case-insensitive prefix filter.
    #[test]
    fn suggestions_filter_by_prefix_ignoring_case() {
        let candidates = ["Global", "local", "Trade"];
        assert_eq!(suggest("", &candidates), ["Global", "local", "Trade"]);
        assert_eq!(suggest("l", &candidates), ["local"]);
        assert_eq!(suggest("LO", &candidates), ["local"]);
        assert!(suggest("zzz", &candidates).is_empty());
    }

    /// The whole current token is replaced, matching the WIT request contract.
    #[test]
    fn suggestions_replace_the_current_token() {
        let suggestions = suggest_matching(
            &SuggestionRequest {
                input: "/trchat channel join gl".to_string(),
                cursor: 23,
                start: 21,
                remaining: "gl".to_string(),
            },
            ["global".to_string()].into_iter(),
        );
        assert_eq!(suggestions.start, 21);
        assert_eq!(suggestions.length, 2);
    }

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
