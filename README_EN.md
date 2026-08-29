# TrChat Mod — NeoForge / Fabric Multi-Version

English | [简体中文](README.md)

A multi-loader, multi-version server-side port of the TrChat Bukkit plugin. Supports the following loaders and Minecraft versions:

| Loader | Versions |
| --- | --- |
| NeoForge | 1.21.1 (21.1.233+), 1.21.11 (21.1.x), 26.1.2, 26.2 |
| Fabric | 1.21.1, 1.21.11, 26.1.2, 26.2 |

It does not include Bukkit, BungeeCord, Velocity, plugin-message proxy transport, or DiscordSRV. Redis is the only cross-server transport and remains wire-compatible with Bukkit TrChat `2.4.9`.

Author and maintainer: [Baymaxawa](https://www.youtube.com/@baymaxawa). Original TrChat authors: Arasple and ItsFlicker. This port follows the newer branch maintained by ItsFlicker.

Repository and issues: [Aruvelut-123/TrChat-Mod](https://github.com/Aruvelut-123/TrChat-Mod).

## Requirements

| Version family | Java | Loader version |
| --- | --- | --- |
| 1.21.x | 21 | NeoForge 21.1.233+ / Fabric Loader 0.16.0+ |
| 26.x | 25 | NeoForge / Fabric + Fabric API (corresponding version) |

Server-side installation only; vanilla clients can join directly.

## Features

- Bukkit-style multi-channel YAML with conditions, priorities, prefixes, message bodies, suffixes, hover/click actions, ranges, commands, and chat-prefix bindings.
- Automatically creates `Normal`, `Global`, `Staff`, and `Private` channels.
- Generates a bilingual, full-field `Example.yml` reference that is never registered as a channel.
- Implements the documented Player and Server placeholders, including dynamic permissions, effects, enchantments, world population, time, countdowns, and TPS.
- Bukkit-style `function.yml`: player/all mentions, item/inventory/Ender Chest sharing, command control, custom regex components, and Action/Actions.
- Bukkit-style `filter.yml`: local and cloud word lists, allowlists, punctuation ignoring, and chat/sign filtering.
- Mutes, shadow mutes, private-chat spying, global mute, reply relationships, and persistent local/cross-server player ignores.
- Player-selectable chat colors through `trchat.color.<0-9a-f>` permissions and an administrator clear-chat command.
- Daily plain-text public/private chat logs with configurable formats and retention.
- Automatic client-locale selection for `lang/zh_CN.yml`, `en_US.yml`, and `es_ES.yml`, with support for additional language files.
- YAML and language synchronization that adds missing keys, removes unknown keys, and retains user values for known keys.
- Local `data.db` persistence, switchable through `datasource.yml` to SQLite, MySQL, MariaDB, or custom JDBC.
- Redis reconnection and Bukkit-compatible broadcast, private-message, online-player-list, and global-mute messages.
- Built-in GitHub Release update checks.

## Loader differences

| Feature | NeoForge | Fabric |
| --- | --- | --- |
| Permission system | NeoForge PermissionAPI (node-based) | Built-in OP level check (no permission API; LuckPerms Fabric recommended) |
| Configuration format | TOML (ModConfigSpec auto-generated) | YAML (manual editing, loader-agnostic) |
| Command interception | CommandEvent (server command controller) | Not supported (alias routing works via registered commands) |
| Anvil rename filtering | AnvilUpdateEvent | Not supported (Fabric API does not provide anvil events) |
| Config migration | Auto-migrate from legacy `trchat-neoforge` directory | Auto-migrate from legacy `trchat-neoforge` directory |

### Fabric notes

- Fabric has no built-in permission API. `TrChatPermissions.check()` requires OP level 2 for administrative nodes (`trchat.admin`, `trchat.command.*`) and grants all other nodes by default. Install LuckPerms Fabric for full permission management.
- Fabric configuration is stored at `config/trchat/settings.yml` (YAML format), not TOML. Create it manually on first launch.
- The server command controller (which intercepts command execution on NeoForge) is not available through Fabric API and is therefore disabled.
- Anvil rename filtering is not available through Fabric API and is therefore disabled.

## Configuration directory

Generated on first launch:

```text
config/trchat/
├── settings.toml         (NeoForge) or settings.yml (Fabric)
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

## Redis and Bukkit interoperability

Edit the configuration file. Each server must use a unique `serverId` (the server port is recommended). The Bukkit TrChat server should connect to the same Redis database and keep the `trchat-message` channel.

Public/global chat, private messages, private-spy, online-player-list, and global-mute can work across Bukkit/NeoForge/Fabric servers. Item, inventory, and Ender Chest snapshots are always local.

## Building and testing

This project uses [Stonecutter](https://stonecutter.kikugie.dev/) to manage multi-version multi-loader differences. The required JDK version is selected automatically.

```powershell
# Build and test all 8 nodes
.\gradlew test

# Build a specific node
.\gradlew :1.21.1-fabric:compileJava
.\gradlew :26.2-neoforge:test
```

Build artifacts are in `versions/<version-loader>/build/libs/`.

## License

This project retains the original [MIT License](LICENSE).