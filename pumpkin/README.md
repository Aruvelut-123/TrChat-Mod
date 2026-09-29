# TrChat for PumpkinMC（实验性）

> ⚠️ **实验性（Experimental）**：本目录是 TrChat v2 对
> [PumpkinMC](https://pumpkinmc.org)（Rust 实现的 Minecraft 服务器）的独立实验性移植，
> 处于 **WIP** 状态，功能为最小可用核心，不保证生产可用。

## 这是什么

TrChat 是一个多平台聊天插件（Bukkit / Bungee / Velocity）。PumpkinMC 的插件机制与
Bukkit 完全不同——插件是 **WASM Component**（Rust / Go / Kotlin 均可，本移植使用
**Rust**，Pumpkin 官方首选语言）。因此本目录是一个 **独立 Rust crate**，不属于
Gradle 构建，与仓库根目录的 Kotlin 模块（`src/`、`versions/`）并存。

| 维度 | Bukkit 版（TrChat v2） | Pumpkin 版（本目录） |
| --- | --- | --- |
| 语言 | Kotlin | Rust（`pumpkin-plugin-api`） |
| 加载机制 | `plugin.yml` + Java 类加载 | WASM Component，放入服务器 `plugins/` |
| 聊天事件 | `AsyncChatEvent`（Paper） | `PlayerChatEvent`（WIT 事件，可取消、可改消息） |
| 配置 | `config.yml`（YAML） | `plugins/data/trchat/config.json`（JSON） |
| 互通 | Redis `trchat-message` 协议 | **规划中**（见下方 Roadmap） |

## 构建

```bash
# 安装 wasm 目标（Rust 1.97+）
rustup target add wasm32-wasip2

# 编译
cargo build --release --target wasm32-wasip2

# （可选）用 wasm-tools 校验产物确实是 WASM Component
wasm-tools component wit target/wasm32-wasip2/release/trchat_pumpkin.wasm
```

> 说明：在当前的 rustc（1.97+，wasm32-wasip2）+ `pumpkin-plugin-api` 组合下，
> `cargo build` 的产物**本身就是 WASM Component**（magic `\0asm\x0d\x00\x01\x00`），
> 无需再执行 `wasm-tools component new`。

把 `target/wasm32-wasip2/release/trchat_pumpkin.wasm` 放入 Pumpkin 服务器的 `plugins/`
目录即被加载。GitHub Actions 构建产物（`TrChat-pumpkin-<运行编号>` artifact）也可直接使用。

## 功能（当前）

* 拦截 `PlayerChatEvent`（最高优先级、阻塞模式）
* 按 `config.json` 中的 `format` 模板渲染聊天消息（支持 `{player}` / `{message}` / `{channel}` 占位符）
* 将渲染结果广播给所有在线玩家，并抑制服务器默认聊天
* **频道系统**：`channels[].prefixes` 前缀路由（最长前缀优先，对齐 Bukkit 版 `ChannelManager.byPrefix`）、`is_default` 回退频道、`Join-Permission` 发言权限、`DISTANCE` 说话半径
* **消息守卫**：`messageMaxLength` 长度限制、`cooldownMillis` 冷却、`antiRepeatSimilarity`/`antiRepeatPeriodMillis` 反重复、全局禁言（`/trchat muteall`）、单玩家禁言（`/trchat mute/unmute`）、忽略（`/trchat ignore`）
* **过滤**：`blockedWords` + `filterReplacement` 敏感词过滤（大小写不敏感、等长替换）
* **语言**：`lang/` 语言表（内置 `en_us` / `zh_cn` / `es_es`），回退链 玩家语言 → 默认语言 → `en_us` → 原始 key
* **命令**：`/trchat`（reload / version / muteall / mute / unmute / ignore / channel）、`/channel <id>`、`/msg <目标> <消息>`（别名 `/tell`）
* **私聊**：`msg.sender` / `msg.receiver` 模板渲染，遵循忽略列表
* **玩家数据**：`SessionPlayers` 会话注册表（活跃频道、已加入频道、禁言、忽略、全局禁言）
* **权限**：命令注册权限（`trchat.use`）与 `CommandSender::has_permission` 管理权限检查（`trchat.admin`）、频道 `Join-Permission`
* **配置热重载**：`/trchat reload` 重新读取数据目录中的 YAML（`settings.yml` + `channels/` + `lang/` + `filter.yml` + `function.yml` + `special-chars.yml`）
* 声明了 Redis 互通所需的全部网络权限（`network.tcp.*`、`network.dns`、`network.loopback`）

## 配置

配置文件全部为 **YAML**，位于插件数据目录（`plugins/data/trchat/`）下的 `settings.yml`、
`channels/<Id>.yml`、`lang/<locale>.yml`、`filter.yml`、`function.yml`、`datasource.yml`
与 `special-chars.yml`，与 Mod 端 `config/trchat/` 的布局一一对应
（键名也保持相同，便于两端共享同一套配置）。**文件不存在时**（首次启动，或管理员删除后
重启）插件会自动从内置默认值写入并创建，方便直接编辑；已存在的文件**不会被覆盖**。

```text
plugins/data/trchat/
├── settings.yml          # 全局：chat.serverId / defaultLanguage / cooldown /
│                         #       antiRepeat.* / filter.* / globalPrefix / plain 格式 /
│                         #       msg.sender / msg.receiver / serverName
├── channels/             # 每文件一个频道（Normal / Global / Staff / Private / …）
│   ├── Normal.yml        #   Options / Bindings(Prefix) / Formats / Sender / Receiver / Console
│   ├── Global.yml        #   Prefix: ['!all']  + Command: ['global', …]
│   └── …
├── lang/                 # 每文件一个语言表（en_US / zh_CN / es_ES / …）
│   ├── en_US.yml
│   └── …
├── filter.yml            # 聊天过滤器：Enable(Chat/Sign/Anvil) + Local 敏感词 +
│                         #   Ignored-Punctuations 跳过标点 + WhiteList 白名单 + Replacement
├── function.yml          # 命令控制器规则 + 内置/自定义聊天功能（Mention / Item-Show / …）
├── datasource.yml        # 数据源（SQLite / MySQL / MariaDB / PostgreSQL / JDBC，解析保留）
└── special-chars.yml     # 资源包特殊字符表（彩色 emoji 白名单 + 颜色包裹）
```

* **频道路由**：`Bindings.Prefix` 匹配（最长前缀优先），未匹配的消息落入自动加入
  （`Options.Auto-Join: true`）的默认频道；`Private.yml` 绑定 `/msg` 等命令与
  `msg.sender` / `msg.receiver` 模板。
* **语言回退链**：玩家语言 → `chat.defaultLanguage` → `en_US` → 原始 key。
* 数据目录中的同名 YAML **覆盖**内置默认值（首次启动写入的副本就是操作员编辑的版本）。

## 命令

| 命令 | 权限 | 说明 |
| --- | --- | --- |
| `/trchat reload` | `trchat.admin` | 重新读取 `settings.yml`、`channels/`、`lang/`、`filter.yml`、`function.yml` 与 `special-chars.yml` |
| `/trchat version` | `trchat.use` | 显示插件版本 |
| `/trchat muteall` | `trchat.admin` | 全局禁言开关 |
| `/trchat mute <玩家>` | `trchat.admin` | 禁言一名玩家 |
| `/trchat unmute <玩家>` | `trchat.admin` | 解除禁言 |
| `/trchat ignore <玩家>` | `trchat.use` | 忽略/取消忽略玩家 |
| `/trchat channel <id>` | `trchat.use` | 切换活跃频道 |
| `/channel <id>` | `trchat.use` | 切换活跃频道（别名） |
| `/msg <目标> <消息>` | `trchat.use` | 私聊（别名 `/tell`） |

## Roadmap（实验阶段后续）

- [x] 频道系统：前缀路由（`#global` / `@local`），对齐 Bukkit 版 `Channel` 语义
- [x] 私聊命令 `/msg`（`PlayerCommandPreprocessEvent` 拦截）
- [x] 权限节点注册（`trchat.use` / `trchat.admin`）
- [ ] Redis 跨服互通：监听 `trchat-message` 频道，与现有 Bukkit/Bungee/Velocity 聊天体系打通
  （当前 `pumpkin-plugin-api` 稳定版未暴露网络客户端接口，WASI 沙箱内 TCP 行为需等 API 提供后实现；插件已声明 `network.tcp.connect` 权限）
- [ ] 更新检查（Bukkit 版通过 HTTP 请求 SpigotMC API，Pumpkin 无对应端点，需自建）
- [x] 特殊字符（`special-chars.yml` 彩色 emoji 白名单 + 颜色包裹）与内置占位符
  （`{player}` / `{message}` / `{server}` / `{world}` / `{target}` / `{time}` 等）
- [x] 配置解析覆盖 Mod 全量 YAML：`function.yml`（命令控制器 + 内置/自定义功能）与
  `datasource.yml`（数据源，解析保留待接线）
- [x] 聊天过滤器（`filter.yml`：`Enable.Chat` + `Local` 敏感词 + `Ignored-Punctuations`
  跳过标点 + `WhiteList` 白名单 + `Replacement` 全角归一化），与 `settings.yml`
  `blockedWords` 双层过滤，对齐 Mod `FilterService` / `MessageGuard` 管道

## 说明

* Pumpkin 插件 API 锁定为**最新 stable release** 标签 `0.2.0+26.3-26.51`
  （commit `204a94e`）。不跟随 master/nightly：nightly 在下一次 stable 发布前
  会有大量破坏性 WIT 变更，插件会随时无法加载。
* 事件、命令、权限等 WIT 定义位于
  `crates/pumpkin-plugin-wit/v0.1/`（[Pumpkin-MC/Pumpkin](https://github.com/Pumpkin-MC/Pumpkin) 仓库内）。
