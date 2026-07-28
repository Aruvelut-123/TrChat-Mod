# TrChat NeoForge

TrChat 的 NeoForge 1.21.1 服务端移植版。项目仅保留 NeoForge 实现，跨服传输仅支持 Redis，并与未维护的 TrChat Bukkit `2.4.9` Redis 协议保持兼容。

## 运行要求

- Minecraft `1.21.1`
- NeoForge `21.1.233+`（当前使用 `21.1.244` 编译）
- Java `21`
- 仅服务端安装，原版客户端无需安装本模组

## 已实现

- 原生 NeoForge 聊天事件接管
- 本服聊天与 Redis 全服聊天
- 与 Bukkit TrChat 2.4.9 双向广播、私聊、玩家列表和全服静音同步
- 格式占位符：`%player%`、`%display_name%`、`%message%`、`%server%`
- `&` / `§` 传统颜色代码
- 聊天冷却、相似内容防刷、长度限制和敏感词替换
- `/msg`、`/tell`、`/w`、`/r` 等私聊命令
- `/global`、`/all`、`/shout` 与默认 `!all` 全服聊天
- Redis 断线重连，无额外 Redis 客户端依赖

## 已移除

- Bukkit、BungeeCord 和 Velocity 运行模块
- BungeeCord/Velocity 插件消息通道
- DiscordSRV 功能与依赖
- TabooLib、Bukkit NMS、PlaceholderAPI 及其他 Bukkit 插件 Hook

物品栏、末影箱和物品展示使用 Bukkit 专属序列化格式，无法安全地跨平台还原，因此不会通过 Redis 处理；普通聊天、全服聊天、私聊、在线玩家列表和全服静音可双向互通。

## 安装与配置

1. 将 `build/libs/trchat_neoforge-2.4.9-neoforge.1.jar` 放进 NeoForge 服务端的 `mods` 目录。
2. 首次启动后编辑 `config/trchat-neoforge.toml`。
3. 将 `[redis].enabled` 设为 `true`，填写 Redis 地址、认证和数据库。
4. `chat.serverId` 必须在每台服务端上唯一。为了兼容 Bukkit，请直接填写该服务端端口。
5. 所有互通服务端必须使用同一 Redis 数据库与频道 `trchat-message`。

NeoForge 端示例：

```toml
[chat]
serverId = 25566
globalPrefix = "!all"
format = "&7<%player%>&f %message%"
globalFormat = "&8[&bGlobal&8] &7<%player%>&f %message%"

[redis]
enabled = true
host = "127.0.0.1"
port = 6379
username = ""
password = ""
database = 0
channel = "trchat-message"
```

Bukkit TrChat 2.4.9 的 `plugins/TrChat/settings.yml`：

```yaml
Options:
  Proxy: REDIS

Redis:
  enabled: true
  host: 127.0.0.1
  port: 6379
  user: ~
  password: ~
```

修改 Redis 配置后重启服务端，或执行 `/trchat redis reconnect`。可用 `/trchat status` 查看连接状态。

## 命令

| 命令 | 用途 |
| --- | --- |
| `/trchat status` | 查看 Redis 与全服静音状态 |
| `/trchat redis reconnect` | 重新建立 Redis 连接（需要管理员权限） |
| `/trchat mute on\|off` | 开关全服静音并同步到 Bukkit（需要管理员权限） |
| `/trchat msg <玩家> <内容>` | 发送私聊 |
| `/trmsg <玩家> <内容>` | 私聊的无冲突命令名 |
| `/trreply <内容>` | 回复上一位私聊发送者 |
| `/global <内容>` | 发送 Redis 全服聊天 |

同时注册 Bukkit 版常用别名：`/msg`、`/message`、`/tell`、`/whisper`、`/w`、`/r`、`/reply`、`/all` 和 `/shout`。若整合包中的其他模组占用相同命令，请使用 `trmsg`、`trreply` 或 `trchat` 子命令。

## 在 D 盘构建与测试

本次移植工作副本位于 `D:\TrChat-Neoforge`。PowerShell：

```powershell
Set-Location D:\TrChat-Neoforge
.\scripts\build.ps1 -Clean
```

也可以直接运行：

```powershell
$env:JAVA_HOME = 'C:\Program Files\Microsoft\jdk-21.0.10.7-hotspot'
.\gradlew.bat clean build
```

产物位于 `build/libs/`。开发服务端可通过 `.\scripts\run-server.ps1` 启动；第一次运行时显式传入 `-AcceptEula` 才会写入开发目录的 EULA 文件。

## 同步 Bukkit 上游

NeoForge 与 Bukkit 的平台实现已经分离，不应把 Bukkit 源码直接合并回来。仓库记录最后检查的上游提交，运行：

```powershell
.\scripts\sync-upstream.ps1
```

脚本会获取 `TrPlugins/TrChat` 的 `v2` 分支，并只显示从上次同步点开始、与 Redis 协议和聊天行为有关的提交及差异。人工移植相关协议变化并完成测试后，再使用：

```powershell
.\scripts\sync-upstream.ps1 -MarkSynced
```

更新同步基线。这样后续可以跟进上游，同时不会重新引入 Bukkit、代理端或 DiscordSRV 代码。

## 协议说明

Redis 频道默认为 `trchat-message`，载荷保持 Bukkit 2.4.9 使用的 JSON 包装：

```json
{"data":["BroadcastRaw","<uuid>","<component-json>","","true","","<fallback>"]}
```

协议编解码测试位于 `src/test/java`。CI 会在 Java 21 上执行 `gradlew build` 并上传 NeoForge JAR。

## 许可证

沿用原项目的 [MIT License](LICENSE)。
