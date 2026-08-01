# TrChat NeoForge 1.21.1

TrChat 的纯 NeoForge 服务端移植版。仅支持 Minecraft `1.21.1` 与 NeoForge `21.1.233+`，不包含 Bukkit、BungeeCord、Velocity、插件消息代理或 DiscordSRV；跨服只通过 Redis，并保持与 Bukkit TrChat `2.4.9` 的聊天协议互通。

NeoForge 作者与维护者：[Baymaxawa](https://space.bilibili.com/475655508)；原版 TrChat 作者：Arasple。

项目仓库与 Issue：[Aruvelut-123/TrChat-Neoforge](https://github.com/Aruvelut-123/TrChat-Neoforge)。

## 运行要求

- Minecraft `1.21.1`
- NeoForge `21.1.233+`（项目当前使用 `21.1.244` 编译）
- Java `21`
- 只需安装在服务端，原版客户端可以直接加入

## 功能

- Bukkit 风格的多频道 YAML：条件、优先级、前缀/正文/后缀、悬浮与点击动作、范围、命令和聊天前缀绑定。
- 自动创建 `Normal`、`Global`、`Staff`、`Private`、`Server` 频道。
- `Server` 自动接管后台与 `/say`，格式由 `Server.yml` 控制，并在代码层强制禁止 Redis 转发。
- 自动创建双语全字段 `Example.yml`，但加载器永远不会将它注册成频道。
- 完整实现用户所列的 Player 与 Server 占位符，包括动态权限、药水、附魔、世界在线人数、时间、倒计时和 TPS。
- Bukkit 风格 `function.yml`：玩家/全体艾特、物品、背包、末影箱、命令控制、自定义正则组件与 Action/Actions。
- Bukkit 风格 `filter.yml`：本地词库、云词库、白名单、忽略标点、聊天/告示牌/铁砧过滤。
- 普通禁言、ShadowMute、私聊监听、全服禁言、回复关系，以及可持久化的本地/跨服玩家屏蔽。
- 与上游一致的玩家聊天颜色选择（`trchat.color.<0-9a-f>` 权限）和管理员清屏命令。
- 普通聊天与私聊按日写入纯文本日志，可配置格式和自动保留天数。
- 按客户端语言自动选择 `lang/zh_CN.yml`、`en_US.yml`、`es_ES.yml`，并支持自行增加语言文件。
- 语言文件的 `Placeholder-Translations` 可对预先列明的纯英文占位符结果做精确本地化；数字、混合文本、非英文结果及玩家名等动态值保持原样。
- YAML 配置与语言文件加载时自动补齐缺失项、删除未知项，并保留所有已知项的用户值。
- 本地 `data.db` 持久化；`datasource.yml` 可切换 SQLite、MySQL、MariaDB 或自定义 JDBC。三种常用驱动均已内嵌。
- Redis 自动重连，以及与 Bukkit 版的广播、私聊、在线玩家列表和全服禁言协议互通。
- 内置 GitHub Release 更新检查器：启动后检查、默认每 15 分钟复查，并向后台与 `trchat.admin` 管理员发送短版可点击提醒。

## 配置目录

首次启动后生成：

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
    ├── Server.yml
    └── Example.yml
```

频道文件沿用 Bukkit 版的大部分结构。`Options.Proxy` 在本项目中仅表示“使用 Redis 互通”，不是 BungeeCord/Velocity 代理连接。新增、修改或移除频道命令绑定后，执行 `/trchat reload` 即可即时刷新，无需使用 Minecraft 全局 `/reload`。

完整配置项与中英文注释见自动生成的 `channels/Example.yml`。占位符参考链接：

- [Player placeholders](https://wiki.placeholderapi.com/users/placeholder-list/minecraft/#player)
- [Server placeholders](https://wiki.placeholderapi.com/users/placeholder-list/minecraft/#server)

除这两组原生占位符外，还支持 `%message%` 与私聊中的 `%trchat_toplayer%`。其他 Bukkit 插件扩展变量在纯 NeoForge 环境中无法解析，会替换为空文本。

只有代码预先标记为可本地化的布尔状态、游戏模式、方向、世界类型及倒计时错误等占位符，且完整结果全部由英文字母、空格、下划线或连字符组成时，才会查找当前语言文件的 `Placeholder-Translations`。三份默认语言文件已列出允许修改的完整映射；该节点不是动态扩展区，未知键会被配置检查器删除。

## Redis 与 Bukkit 互通

编辑 `settings.toml`：

```toml
[chat]
serverId = 25566
serverName = "模组服"
defaultLanguage = "zh_CN"

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

每台服务端的 `serverId` 必须唯一，建议直接使用服务端端口。Bukkit TrChat 端应连接同一 Redis 数据库，并保留 `trchat-message` 频道。修改连接参数后重启，或执行 `/trchat redis reconnect`。

普通/全服聊天、私聊、私聊监听、在线玩家列表与全服禁言可以跨 Bukkit/NeoForge 互通。`Server` 频道以及物品、背包和末影箱快照始终只在本 NeoForge 服务端处理；聊天中展示原版物品时可以跨服，展示模组物品时消息会限制在本服，避免 Paper 反序列化未知注册键时报错。跨服私聊若包含模组物品则拒绝发送并提示发送者。

## 数据库

默认 `datasource.yml` 使用：

```yaml
Type: SQLite
SQLite:
  File: data.db
```

改为 `Type: MySQL` 或 `Type: MariaDB` 后，填写文件中对应的 Host、Port、Database、User、Password 与 Parameters。连接示例和中英文说明已写在默认配置里。自定义 JDBC 可填写 Driver 与 Url，但驱动必须能在运行环境中被加载。

数据库保存玩家禁言截止时间、原因、ShadowMute、私聊监听开关、频道状态、屏蔽列表和聊天颜色。SQLite 文件位于 `config/trchat-neoforge/data.db`。

聊天日志配置位于 `settings.toml` 的 `[logging]`：`normalFormat`、`privateFormat` 和 `retentionDays`。`retentionDays = 0` 表示不自动删除。

## function.yml

内置功能：

- `Mention` / `Mention-All`
- `Item-Show`
- `Inventory-Show`
- `EnderChest-Show`
- `Command-Controller`
- `Custom` 正则替换与文本、hover、url、command、suggest、copy

`Action` 与 `Actions` 支持：

```yaml
Action:
  - 'console: give {player} minecraft:diamond 1'
  - 'player: me used a chat function'
  - 'message: &a动作执行成功'
  - 'sound: minecraft:block.note_block.pling 1 1'
```

也兼容 `command "..." as console/player` 与 `tell "..."` 写法。变量包括 `{player}`、`{message}` 和 `{0}`。

背包、末影箱和物品 UI 使用只读快照，五分钟后失效，点击不会修改原玩家物品。

## filter.yml

- `Enable.Chat`：替换聊天中的敏感词。
- `Enable.Sign`：检查已加载区块中的告示牌正反面。
- `Enable.Anvil`：检测到敏感重命名时取消铁砧输出。
- `Cloud-Thesaurus`：异步下载 JSON 词库并缓存到 `filters/`。
- `Local`、`Ignored-Punctuations`、`WhiteList`、`Replacement`：与 Bukkit 配置结构一致。

NeoForge 1.21.1 的铁砧事件不能安全地只改写重命名文本而不替换整个输出，因此敏感名称采用“取消操作”而不是局部打码。

## 命令

| 命令 | 用途 |
| --- | --- |
| `/trchat status` | 查看频道、Redis 与全服禁言状态 |
| `/trchat status <玩家>` | 查看在线玩家的频道、禁言、ShadowMute、监听、OP、延迟与游戏模式 |
| `/trchat reload` | 局部重载频道、命令绑定、function、filter、语言和 Redis |
| `/trchat redis reconnect` | 重新连接 Redis |
| `/trchat mute on\|off` | 开关并同步全服禁言 |
| `/trchat mute player <玩家> <时长> [原因]` | 普通禁言，支持 `30s`、`5m`、`1h30m`、`7d`、`permanent` |
| `/mute <玩家> <时长> [原因]` | 普通禁言快捷命令 |
| `/trchat unmute <玩家>` | 解除普通禁言 |
| `/trchat shadowmute <玩家> [on\|off]` | 切换影子禁言 |
| `/shadowmute <玩家> [on\|off]` | ShadowMute 快捷命令 |
| `/trchat spy [on\|off]` | 切换私聊监听 |
| `/ignore <玩家> [on\|off]`、`/trignore` | 屏蔽或恢复接收本地/跨服玩家消息 |
| `/ignorelist` | 查看已屏蔽玩家；离线后仍可按列表中的名字解除 |
| `/trchat color <0-9a-f\|reset>` | 选择或重置聊天颜色 |
| `/trchat clear <玩家\|*>` | 为一个玩家或全体玩家清屏 |
| `/trchat msg <玩家> <内容>` | 发送私聊 |
| `/tell`、`/msg` 等 Private 频道绑定 | 通过 Private 频道发送本服或跨服私聊 |
| `/r <内容>`、`/reply <内容>` | 回复最后一个向你发送私聊的玩家 |
| `/trchat channel join <频道>` | 加入或离开频道 |
| `/say <内容>` | 通过本地 `Server` 频道格式化发送 |

另有 `/trmsg`、`/trreply`、`/trmute`、`/trunmute`、`/trshadowmute`、`/trspy`，以及频道 YAML 中定义的动态命令。

## 在 D 盘构建和测试

本项目工作目录为 `D:\TrChat-Neoforge`：

```powershell
Set-Location D:\TrChat-Neoforge
.\scripts\build.ps1 -Clean
```

产物位于 `build/libs/`。开发服务端可使用：

```powershell
.\scripts\run-server.ps1 -AcceptEula
```

脚本只在明确传入 `-AcceptEula` 时写入开发服 EULA 文件。

## 创建 Release

安装并登录 [GitHub CLI](https://cli.github.com/)，同时把 `git-cliff` 加入 PATH，然后运行：

```powershell
.\scripts\release.ps1 -Tag v2.4.9.5
```

脚本会检查干净工作区、完整构建、通过 `git-cliff` 从提交生成更新列表、创建并推送 tag，再用 `gh release create` 上传唯一的非 sources JAR。GitHub Release 发布事件也会触发工作流重新构建、更新提交列表并覆盖上传产物。

后续提交统一使用 Conventional Commits，格式与允许类型见 [CONTRIBUTING.md](CONTRIBUTING.md)。CI 会校验新推送及 Pull Request 中的提交信息。

## 同步 Bukkit 上游

本仓库只保留 NeoForge 源码。使用：

```powershell
.\scripts\sync-upstream.ps1
```

脚本获取 `TrPlugins/TrChat` 的 `v2` 分支，只展示 Redis 协议、频道和聊天行为相关差异。人工移植并验证后执行：

```powershell
.\scripts\sync-upstream.ps1 -MarkSynced
```

这样可持续同步上游行为，同时不会重新引入 Bukkit、代理端或 DiscordSRV。

当前审计基线为 Bukkit TrChat `2.4.9`（`1428f01`）。CMI、DiscordSRV、PlaceholderAPI 等 Bukkit 插件钩子，以及依赖 Paper 客户端消息包重放的接收过滤开关和消息撤回，不属于纯 NeoForge 可等价移植的功能；其余聊天核心能力按 NeoForge API 实现。

## 许可证

沿用原项目 [MIT License](LICENSE)。
