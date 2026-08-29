# TrChat Mod — NeoForge / Fabric / Forge 多版本

[English](README_EN.md) | 简体中文

TrChat Bukkit 插件的多加载器多版本服务端移植版。支持以下加载器与 Minecraft 版本：

| 加载器 | 版本 |
| --- | --- |
| NeoForge | 1.21.1（21.1.233+）、1.21.11（21.1.x）、26.1.2、26.2 |
| Fabric | 1.21.1、1.21.11、26.1.2、26.2 |
| Forge（LTS） | 1.20.1（47.4.0+） |

不包含 Bukkit、BungeeCord、Velocity、插件消息代理或 DiscordSRV；跨服只通过 Redis，并保持与 Bukkit TrChat `2.4.9` 的聊天协议互通。

作者与维护者：[Baymaxawa](https://space.bilibili.com/475655508)；原版 TrChat 作者：Arasple、ItsFlicker。本项目基于 ItsFlicker 维护的较新分支继续移植。

项目仓库与 Issue：[Aruvelut-123/TrChat-Mod](https://github.com/Aruvelut-123/TrChat-Mod)。

## 运行要求

| 版本 | Java | 加载器版本 |
| --- | --- | --- |
| 1.20.1 Forge（LTS） | 17 | Forge 47.4.0+ |
| 1.21.x 系列 | 21 | NeoForge 21.1.233+ / Fabric Loader 0.16.0+ |
| 26.x 系列 | 25 | NeoForge / Fabric + Fabric API 对应版本 |

只需安装在服务端，原版客户端可以直接加入。

## 功能

- Bukkit 风格的多频道 YAML：条件、优先级、前缀/正文/后缀、悬浮与点击动作、范围、命令和聊天前缀绑定。
- 自动创建 `Normal`、`Global`、`Staff`、`Private` 频道。
- 自动创建双语全字段 `Example.yml`，但加载器永远不会将它注册成频道。
- 完整实现 Player 与 Server 占位符，包括动态权限、药水、附魔、世界在线人数、时间、倒计时和 TPS。
- Bukkit 风格 `function.yml`：玩家/全体艾特、物品、背包、末影箱、命令控制、自定义正则组件与 Action/Actions。
- Bukkit 风格 `filter.yml`：本地词库、云词库、白名单、忽略标点、聊天/告示牌过滤。
- 普通禁言、ShadowMute、私聊监听、全服禁言、回复关系，以及可持久化的本地/跨服玩家屏蔽。
- 与上游一致的玩家聊天颜色选择（`trchat.color.<0-9a-f>` 权限）和管理员清屏命令。
- 普通聊天与私聊按日写入纯文本日志，可配置格式和自动保留天数。
- 按客户端语言自动选择 `lang/zh_CN.yml`、`en_US.yml`、`es_ES.yml`，并支持自行增加语言文件。
- YAML 配置与语言文件加载时自动补齐缺失项、删除未知项，并保留所有已知项的用户值。
- 本地 `data.db` 持久化；`datasource.yml` 可切换 SQLite、MySQL、MariaDB 或自定义 JDBC。
- Redis 自动重连，以及与 Bukkit 版的广播、私聊、在线玩家列表和全服禁言协议互通。
- 内置 GitHub Release 更新检查器。

## 加载器差异

| 特性 | NeoForge | Fabric | Forge 1.20.1（LTS） |
| --- | --- | --- | --- |
| 权限系统 | NeoForge PermissionAPI（节点式） | 内置 OP 级别检查（无权限 API，可配合 LuckPerms Fabric 版） | Forge PermissionAPI（节点式） |
| 配置格式 | TOML（ModConfigSpec 自动生成） | YAML（手动编辑，加载器无关） | TOML（ForgeConfigSpec 自动生成） |
| 命令拦截 | CommandEvent（服务端命令控制器） | 不支持（别名路由通过注册命令实现） | CommandEvent（服务端命令控制器） |
| 铁砧过滤 | AnvilUpdateEvent | 不支持（Fabric API 未提供铁砧事件） | AnvilUpdateEvent |
| 配置迁移 | 自动从旧版 `trchat-neoforge` 目录迁移 | 自动从旧版 `trchat-neoforge` 目录迁移 | 自动从旧版 `trchat-neoforge` 目录迁移 |

### Fabric 特别说明

- Fabric 不提供内置权限 API，`TrChatPermissions.check()` 对管理节点（`trchat.admin`、`trchat.command.*`）检查 OP 级别 2，其余节点默认放行。建议安装 LuckPerms Fabric 版获得完整权限管理。
- Fabric 配置位于 `config/trchat/settings.yml`（YAML 格式），而非 NeoForge 的 TOML。首次启动后需手动创建。
- 服务端命令控制器（在 NeoForge 中拦截命令执行）在 Fabric 上无法通过 API 实现，因此不生效。
- 铁砧重命名过滤在 Fabric 上无法通过 API 实现，因此不生效。

### Forge 1.20.1（LTS）特别说明

- Forge 1.20.1 是长期支持版本，运行于 Java 17，与 1.21+ 共享同一套业务逻辑。
- 配置位于 `config/trchat/settings.toml`，由 ForgeConfigSpec 首次启动自动生成。
- 潜影盒等容器物品快照按 NBT 读取，行为与 NeoForge 一致。
- 服务端命令控制器与铁砧过滤通过 Forge 事件实现，与 NeoForge 行为一致。

## 配置目录

首次启动后生成：

```text
config/trchat/
├── settings.toml         (NeoForge/Forge) 或 settings.yml (Fabric)
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

频道文件沿用 Bukkit 版的大部分结构。`Options.Proxy` 在本项目中仅表示"使用 Redis 互通"，不是 BungeeCord/Velocity 代理连接。

## Redis 与 Bukkit 互通

编辑配置文件。每台服务端的 `serverId` 必须唯一，建议直接使用服务端端口。Bukkit TrChat 端应连接同一 Redis 数据库，并保留 `trchat-message` 频道。

普通/全服聊天、私聊、私聊监听、在线玩家列表与全服禁言可以跨 Bukkit/NeoForge/Fabric 互通。物品、背包和末影箱快照始终只在本服处理。

## 构建和测试

本工程使用 [Stonecutter](https://stonecutter.kikugie.dev/) 管理多版本多加载器差异。构建所需 JDK 版本根据目标版本自动选择。

```powershell
# 全部 9 个节点编译+测试
.\gradlew test

# 仅编译特定节点
.\gradlew :1.21.1-fabric:compileJava
.\gradlew :26.2-neoforge:test
```

产物位于 `versions/<版本-加载器>/build/libs/`。

## 许可证

沿用原项目 [MIT License](LICENSE)。