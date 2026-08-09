# TrChat NeoForge 1.21.1

English | [简体中文](README.md)

A native server-side NeoForge port of TrChat for Minecraft `1.21.1` and NeoForge `21.1.233+`. It does not include Bukkit, BungeeCord, Velocity, plugin-message proxy transport, or DiscordSRV. Redis is the only cross-server transport and remains wire-compatible with Bukkit TrChat `2.4.9`.

NeoForge author and maintainer: [Baymaxawa](https://www.youtube.com/@baymaxawa). Original TrChat authors: Arasple and ItsFlicker. This port follows the newer branch maintained by ItsFlicker.

Repository and issues: [Aruvelut-123/TrChat-Neoforge](https://github.com/Aruvelut-123/TrChat-Neoforge).

## Requirements

- Minecraft `1.21.1`
- NeoForge `21.1.233+` (currently compiled against `21.1.244`)
- Java `21`
- Server-side installation only; vanilla clients can join directly

## Features

- Bukkit-style multi-channel YAML with conditions, priorities, prefixes, message bodies, suffixes, hover/click actions, ranges, commands, and chat-prefix bindings.
- Automatically creates `Normal`, `Global`, `Staff`, and `Private` channels.
- Server `/say` uses the public format of the channel with `Options.Auto-Join`; the console name is localized for each client.
- Generates a bilingual, full-field `Example.yml` reference that is never registered as a channel.
- Implements the documented Player and Server placeholders, including dynamic permissions, effects, enchantments, world population, time, countdowns, and TPS.
- Bukkit-style `function.yml`: player/all mentions, item/inventory/Ender Chest sharing, command control, custom regex components, and Action/Actions.
- Bukkit-style `filter.yml`: local and cloud word lists, allowlists, punctuation ignoring, and chat/sign/anvil filtering.
- Mutes, shadow mutes, private-chat spying, global mute, reply relationships, and persistent local/cross-server player ignores.
- Player-selectable chat colors through `trchat.color.<0-9a-f>` permissions and an administrator clear-chat command.
- Daily plain-text public/private chat logs with configurable formats and retention.
- Automatic client-locale selection for `lang/zh_CN.yml`, `en_US.yml`, and `es_ES.yml`, with support for additional language files.
- Exact localization of approved English-only placeholder results through `Placeholder-Translations`.
- YAML and language synchronization that adds missing keys, removes unknown keys, and retains user values for known keys.
- Local `data.db` persistence, switchable through `datasource.yml` to SQLite, MySQL, MariaDB, or custom JDBC. Common drivers are bundled.
- Redis reconnection and Bukkit-compatible broadcast, private-message, online-player-list, and global-mute messages.
- Runtime detection of E33Chat's optional server component. Recognition templates are derived from the loaded TrChat channel formats, while chat and command handling runs after other mods have observed or rewritten the events.
- Built-in GitHub Release update checks at startup and every 15 minutes by default, with full release notes and clickable links for the console and `trchat.admin` players.

## Mod compatibility

E33Chat is optional. When only the client has E33Chat, no server detection is needed: TrChat sends the same standard Minecraft component to every player. When E33Chat `2.2.6+` is also installed on the server, TrChat reads every loaded channel's `Formats`, `Sender`, and `Receiver` sections at startup and after `/trchat reload`. It derives E33Chat parsing templates from the configured player-name, target-name, and message boundaries, replaces E33Chat's runtime chat-format list with those templates, and broadcasts them to every connected E33Chat client.

The TrChat channel YAML therefore remains the single format source. Players without E33Chat receive the same TrChat-rendered component, and the generated templates are not written to or used to take ownership of `e33chat-server.toml`.

E33Chat's template syntax cannot safely express formats with no separator at all between the player name and message body, or formats that repeat the player name. Those layouts are skipped because their fields cannot be separated reliably; the original TrChat message is still delivered normally.

## Configuration layout

The first launch creates:

```text
config/trchat-neoforge/
├── settings.toml
├── datasource.yml
├── data.db
├── function.yml
├── filter.yml
├── filters/
├── lang/
│   ├── zh_CN.yml
│   ├── en_US.yml
│   └── es_ES.yml
├── logs/
│   └── yyyy-MM-dd.txt
└── channels/
    ├── Normal.yml
    ├── Global.yml
    ├── Staff.yml
    ├── Private.yml
    └── Example.yml
```

Channel files retain most of the Bukkit layout. In this project, `Options.Proxy` means Redis forwarding only; it does not enable a BungeeCord or Velocity link. Run `/trchat reload` after adding, changing, or removing channel command bindings. A global Minecraft `/reload` is not required.

The generated `channels/Example.yml` documents every field in Chinese and English. Placeholder references:

- [Player placeholders](https://wiki.placeholderapi.com/users/placeholder-list/minecraft/#player)
- [Server placeholders](https://wiki.placeholderapi.com/users/placeholder-list/minecraft/#server)

In addition to those native groups, `%message%` and the private-chat `%trchat_toplayer%` placeholder are supported. Bukkit plugin-extension placeholders cannot be resolved in a pure NeoForge environment and become empty text.

Only Boolean states, game modes, directions, world types, countdown errors, and other values explicitly marked by the code can use `Placeholder-Translations`. Localization is attempted only when the entire result consists of English letters, spaces, underscores, or hyphens. Unknown translation keys are removed by configuration synchronization. Local messages are rendered for each recipient's locale. Bukkit-compatible Redis packets can carry only one rendered component, so cross-server messages use `defaultLanguage`.

## Redis and Bukkit interoperability

Edit `settings.toml`:

```toml
[chat]
serverId = 25566
serverName = "Modded Server"
defaultLanguage = "en_US"

[updates]
enabled = true
intervalMinutes = 15

[redis]
enabled = true
host = "127.0.0.1"
port = 6379
username = ""
password = ""
database = 0
channel = "trchat-message"
```

Every server must use a unique `serverId`; using the server port is recommended. Bukkit TrChat must connect to the same Redis database and retain the `trchat-message` channel. Restart after changing connection settings, or run `/trchat redis reconnect`.

Normal/global chat, server `/say`, private chat, private-chat spying, online-player lists, and global mute interoperate across Bukkit and NeoForge. Item, inventory, and Ender Chest snapshots always stay on the local NeoForge server. Vanilla item displays may cross servers; messages containing modded items remain local so Paper never attempts to deserialize an unknown registry key. A cross-server private message containing a modded item is rejected with feedback to the sender.

## Database

The default `datasource.yml` uses:

```yaml
Type: SQLite
SQLite:
  File: data.db
```

For `Type: MySQL` or `Type: MariaDB`, fill in Host, Port, Database, User, Password, and Parameters. The generated file includes bilingual examples. Custom JDBC accepts Driver and Url values, but its driver must be available at runtime.

The database stores mute expiry/reason, shadow-mute and private-spy states, channel membership, ignored players, and chat color. The default SQLite file is `config/trchat-neoforge/data.db`.

Chat logging is configured under `[logging]` in `settings.toml`: `normalMessageFormat`, `privateMessageFormat`, and `retentionDays`. Set `retentionDays = 0` to disable automatic deletion.

## function.yml

Built-in functions:

- `Mention` / `Mention-All`
- `Item-Show`
- `Inventory-Show`
- `EnderChest-Show`
- `Command-Controller`
- `Custom` regex replacements with text, hover, url, command, suggest, and copy actions

`Action` and `Actions` support:

```yaml
Action:
  - 'console: give {player} minecraft:diamond 1'
  - 'player: me used a chat function'
  - 'message: &aAction completed'
  - 'sound: minecraft:block.note_block.pling 1 1'
```

The `command "..." as console/player` and `tell "..."` forms are also accepted. Variables include `{player}`, `{message}`, and `{0}`.

Inventory, Ender Chest, and item interfaces use read-only snapshots. They expire after five minutes, and clicking cannot modify the source inventory.

## filter.yml

- `Enable.Chat`: replace sensitive terms in chat.
- `Enable.Sign`: inspect both sides of signs in loaded chunks.
- `Enable.Anvil`: cancel anvil output when a sensitive rename is detected.
- `Cloud-Thesaurus`: asynchronously download JSON word lists and cache them under `filters/`.
- `Local`, `Ignored-Punctuations`, `WhiteList`, and `Replacement`: follow the Bukkit configuration structure.

NeoForge 1.21.1 cannot safely rewrite only an anvil's rename text without replacing the complete output stack, so sensitive names cancel the operation instead.

## Commands

| Command | Purpose |
| --- | --- |
| `/trchat status` | Show channel, Redis, and global-mute status |
| `/trchat status <player>` | Show an online player's channel, mute, shadow mute, spy, OP, latency, and game mode |
| `/trchat reload` | Reload channels, command bindings, functions, filters, languages, and Redis |
| `/trchat redis reconnect` | Reconnect Redis |
| `/trchat mute on\|off` | Toggle and synchronize global mute |
| `/trchat mute player <player> <duration> [reason]` | Mute a player; supports `30s`, `5m`, `1h30m`, `7d`, and `permanent` |
| `/mute <player> <duration> [reason]` | Short mute command |
| `/trchat unmute <player>` | Remove a mute |
| `/trchat shadowmute <player> [on\|off]` | Toggle a shadow mute |
| `/shadowmute <player> [on\|off]` | Short shadow-mute command |
| `/trchat spy [on\|off]` | Toggle private-chat spying |
| `/ignore <player> [on\|off]`, `/trignore` | Ignore or restore local/cross-server messages from a player |
| `/ignorelist` | List ignored players, including offline entries that can be restored by name |
| `/trchat color <0-9a-f\|reset>` | Select or reset chat color |
| `/trchat clear <player\|*>` | Clear chat for one player or everyone |
| `/trchat msg <player> <message>` | Send a private message |
| `/tell`, `/msg`, and other Private bindings | Send local or cross-server private messages through the Private channel |
| `/r <message>`, `/reply <message>` | Reply to the most recent private-message sender |
| `/trchat channel join <channel>` | Join or leave a channel |
| `/trchat channel join <channel> <player>` | Move another player into a channel; requires `trchat.command.channel.other` |
| `/trchat channel quit [player]` | Leave the active channel; targeting another player requires permission |
| `/say <message>` | Send through the Auto-Join channel with the localized console name |

Additional aliases include `/trmsg`, `/trreply`, `/trmute`, `/trunmute`, `/trshadowmute`, and `/trspy`, plus dynamic commands declared in channel YAML.

## Building and testing

Enter the repository directory on any platform and use Java 21. On Linux, macOS, and other POSIX shells:

```bash
./gradlew clean test build
```

On Windows PowerShell:

```powershell
.\gradlew.bat clean test build
```

Artifacts are written to `build/libs/`. On Windows, the development server can also be started with:

```powershell
.\scripts\run-server.ps1 -AcceptEula
```

The script writes the development server EULA only when `-AcceptEula` is explicitly supplied.

## Creating a release

Install and authenticate [GitHub CLI](https://cli.github.com/), add `git-cliff` to PATH, then run:

```powershell
.\scripts\release.ps1 -Tag v2.4.10.1
```

The script requires a clean worktree, runs a full build, generates release notes from commits with `git-cliff`, creates and pushes the tag, and uploads the single non-sources JAR through `gh release create`. The GitHub Release workflow rebuilds the project, refreshes the commit list, and replaces the uploaded artifact.

All commits use Conventional Commits. See [CONTRIBUTING.md](CONTRIBUTING.md) for the accepted types and format. CI validates pushed and pull-request commit messages.

## Syncing the Bukkit upstream

This repository contains NeoForge sources only. Run:

```powershell
.\scripts\sync-upstream.ps1
```

The script fetches the `v2` branch of ItsFlicker's `TrPlugins/TrChat` repository and displays only Redis protocol, channel, and chat-behavior differences. After manually porting and verifying changes, run:

```powershell
.\scripts\sync-upstream.ps1 -MarkSynced
```

This keeps upstream behavior synchronized without reintroducing Bukkit, proxy, or DiscordSRV code.

The current audit baseline is Bukkit TrChat `2.4.9` (`1428f01`). Bukkit-only CMI, DiscordSRV, and PlaceholderAPI hooks, receiver-side filtering based on Paper packet replay, and message recall cannot be ported equivalently to pure NeoForge. Other core chat behavior uses NeoForge APIs.

## License

This project retains the original [MIT License](LICENSE).
