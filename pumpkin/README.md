# TrChat for PumpkinMC（实验性）

> ⚠️ **实验性（Experimental）**：本目录是 TrChat v2 对
> [PumpkinMC](https://pumpkinmc.org)（Rust 实现的 Minecraft 服务器）的独立实验性移植。
> 聊天管线、频道、过滤、命令、权限、语言表、更新检查与跨服 Redis 互通
> （`trchat-message` 协议，自研 RESP 客户端）均已按 Mod 逐条对齐。

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
| 配置 | `config/trchat/*.yml`（YAML） | 同一套 YAML，落在插件数据目录下 |
| 互通 | Redis `trchat-message` 协议 | **已实现**：自研 RESP 客户端（`src/redis.rs` + `src/resp.rs`），公共/私聊/全局静音/名单/语言通知五通道对齐 Mod 线协议 |

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
* 按频道 `Formats` 模板渲染聊天消息（支持 `%player_name%` / `%message%` / `%server_name%` 等内置占位符）
* 将渲染结果广播给所有在线玩家，并抑制服务器默认聊天
* **频道系统**：`Bindings.Prefix` 前缀路由（最长前缀优先，对齐 Bukkit 版 `ChannelManager.byPrefix`）、`Options.Auto-Join` 回退频道、`Speak-Condition` / `Join-Permission` 权限、`Target` 说话范围
* **消息守卫**：`messageMaxLength` 长度限制、`cooldownMillis` 冷却、`antiRepeat*` 反重复、`antiHighFrequency*` 高频限制、`antiDuplicate*` 连续重复、全局禁言（`/trchat mute`）、单玩家禁言（`/trchat mute player`、`/trchat unmute`）、影禁言（`/trchat shadowmute`）、忽略（`/trchat ignore`）
* **过滤**：`filter.yml` 的 `Enable.Chat` / `Enable.Sign` / `Enable.Anvil`（聊天、告示牌、铁砧三条管线），`Local` 敏感词 + `Cloud-Thesaurus` 云端词库（经宿主 `wasi:http` 抓取，失败回退 `filters/<hash>.json` 缓存）+ `Ignored-Punctuations` 跳过标点 + `WhiteList` 白名单 + `Replacement`，并与 `settings.yml` 的 `blockedWords` / `filterReplacement` 双层过滤
* **语言**：`lang/` 语言表（内置 `en_US` / `zh_CN` / `es_ES`），回退链 玩家语言 → `chat.defaultLanguage` → `en_US` → 原始 key
* **命令**：`/trchat`（status / reload / redis / mute / unmute / shadowmute / spy / msg / channel / color / clear / ignore / view）、`/msg`（别名 `/tell`、`/trmsg`）、`/trreply`（别名 `/r`、`/reply`）、`/trmute`、`/trunmute`、`/trshadowmute`、`/trspy`、`/ignore`、`/ignorelist` 与 `Bindings.Command` 生成的频道动态别名（`/global`、`/all`、`/shout`、`/staff` …）
* **命令控制器**：`function.yml` 的 `General.Command-Controller` 规则，`/arasple`、`/ver(sion)(s)`、`/help(s)` 由规则匹配后放行
* **私聊**：`%trchat_toplayer%` / `Sender` / `Receiver` 模板渲染，遵循忽略列表，可选私聊监听（`/trchat spy`）
* **聊天功能**：`function.yml` 的 `Mention`、`Item-Show`、背包快照（`/trchat view`）
* **更新检查**：`updates: enabled / intervalMinutes`，经宿主 `wasi:http` 拉取 GitHub release，命中后通知控制台与在线管理员（与 Mod 同款语义版本比较）
* **玩家数据**：`SessionPlayers` 会话注册表（活跃频道、已加入频道、禁言、忽略、全局禁言）
* **权限**：Mod 的 `trchat.*` 节点全集（含 `trchat.color.<0-9a-f>` 共 16 个），注册默认值与 Mod 一致；`reload` / `redis reconnect` 为仅 OP2
* **配置热重载**：`/trchat reload` 重新读取数据目录中的 YAML（`settings.yml` + `channels/` + `lang/` + `filter.yml` + `function.yml` + `special-chars.yml`），输出成功 / 部分失败 / 整体失败三态
* 声明了 Redis 互通所需的网络权限（`network.tcp.*`、`network.dns`、`network.loopback`）与更新检查所需的 `http.outbound`

## 配置

配置文件全部为 **YAML**，位于插件数据目录（`plugins/data/trchat/`）下的 `settings.yml`、
`channels/<Id>.yml`、`lang/<locale>.yml`、`filter.yml`、`function.yml`、`datasource.yml`
与 `special-chars.yml`，与 Mod 端 `config/trchat/` 的布局一一对应
（键名也保持相同，便于两端共享同一套配置）。**文件不存在时**（首次启动，或管理员删除后
重启）插件会自动从内置默认值写入并创建，方便直接编辑；**已存在的文件不会被整体覆盖**，
但 `settings.yml`、`datasource.yml`、`filter.yml` 与 `channels/*.yml` 在解析前会与内置默认
对齐：缺失的键补成出厂值、内置 schema 不认识的键删除，其余取值（含整个列表）保持编辑器里
的内容（对齐 Mod 的 `YamlConfigSynchronizer`；`function.yml` 的键是用户自定义命令、
`lang/*.yml` 是译表，两者只做缺失播种，不会被裁剪）。

```text
plugins/data/trchat/
├── settings.yml          # 全局：chat.*（serverId / serverName / defaultLanguage /
│                         #   globalPrefix / messageMaxLength / cooldownMillis /
│                         #   antiRepeat* / antiHighFrequency* / antiDuplicate* /
│                         #   blockedWords / filterReplacement / disabledWorlds）、
│                         #   logging.*（日志格式与保留天数）、
│                         #   updates.*（enabled / intervalMinutes）、redis.*
├── channels/             # 每文件一个频道（Normal / Global / Staff / Private / …）
│   ├── Normal.yml        #   Options / Bindings(Prefix,Command) / Formats / Sender / Receiver / Console
│   ├── Global.yml        #   Prefix: ['!all']  + Command: ['global', 'all', 'shout']
│   └── …
├── lang/                 # 每文件一个语言表（en_US / zh_CN / es_ES / …）
│   ├── en_US.yml
│   └── …
├── filter.yml            # 聊天过滤器：Enable(Chat/Sign/Anvil) + Local 敏感词 +
│                         #   Ignored-Punctuations 跳过标点 + WhiteList 白名单 + Replacement
├── function.yml          # 命令控制器规则 + 内置/自定义聊天功能（Mention / Item-Show / …）
├── datasource.yml        # 数据源（SQLite / MySQL / MariaDB / PostgreSQL，解析对齐 + 文件落盘）
└── special-chars.yml     # 资源包特殊字符表（彩色 emoji 白名单 + 颜色包裹）
```

* **频道路由**：`Bindings.Prefix` 匹配（最长前缀优先），未匹配的消息落入自动加入
  （`Options.Auto-Join: true`，如 `Normal.yml`）的默认频道；`Private.yml` 绑定
  `/msg` 等命令并用 `Sender` / `Receiver` 模板渲染私聊。
* **语言回退链**：玩家语言 → `chat.defaultLanguage` → `en_US` → 原始 key。
* 数据目录中的同名 YAML **覆盖**内置默认值（首次启动写入的副本就是操作员编辑的版本）。

## 命令

### `/trchat` 子命令

| 命令 | 权限 | 说明 |
| --- | --- | --- |
| `/trchat status` | 无 | 插件概览：版本、频道数、默认频道、Redis / 禁言 / 命令控制状态、在线数 |
| `/trchat status <玩家>` | `trchat.admin` | 该玩家的频道、禁言（含到期与原因）、影禁言、监听、OP 等级 |
| `/trchat reload` | **仅 OP2** | 重读数据目录 YAML，输出成功 / 部分失败（列出失败段）/ 整体失败三态 |
| `/trchat redis reconnect` | **仅 OP2** | 触发 Redis 重连（移植版无 Redis 运行时，仅回显提示键） |
| `/trchat mute` | `trchat.mute` | 切换全服禁言 |
| `/trchat mute on\|off` | `trchat.mute` | 显式设置全服禁言 |
| `/trchat mute player <玩家> <时长> [原因]` | `trchat.mute` | 禁言一名玩家（时长支持 `30s` / `5m` / `1h` / `1d` / `7d` / `permanent` 等，原因省略记 `-`） |
| `/trchat unmute <玩家>` | `trchat.mute` | 解除禁言 |
| `/trchat shadowmute <玩家> [on\|off]` | `trchat.shadowmute` | 影禁言（省略 on/off 即取反） |
| `/trchat spy [on\|off]` | OP2 或 `trchat.spy` | 私聊监听（省略即取反） |
| `/trchat msg <玩家> <消息>` | 无 | 私聊 |
| `/trchat channel join <频道> [玩家]` | 无 / 他人需 `trchat.command.channel.other` | 切换频道 |
| `/trchat channel quit [玩家]` | 无 / 他人需 `trchat.command.channel.other` | 退出频道 |
| `/trchat color <颜色>` | `trchat.command.color` | 设置聊天颜色，`reset` 复位 |
| `/trchat clear <玩家\|*>` | `trchat.command.clear` | 清屏（`*` = 全服） |
| `/trchat ignore <玩家> [on\|off]` | `trchat.command.ignore`（默认开放） | 忽略 / 取消忽略 |
| `/trchat view <快照>` | 无 | 打开只读背包快照 |

### 顶层命令与别名

| 命令 | 权限 | 说明 |
| --- | --- | --- |
| `/msg <目标> <消息>`、`/tell`、`/trmsg` | 无 | 私聊 |
| `/trreply <消息>`、`/r`、`/reply` | 无 | 回复最近私聊对象 |
| `/trmute`、`/mute <玩家> <时长> [原因]` | `trchat.mute` | 禁言（无 `on/off` 分支） |
| `/trunmute <玩家>` | `trchat.mute` | 解除禁言 |
| `/trshadowmute`、`/shadowmute <玩家> [on\|off]` | `trchat.shadowmute` | 影禁言 |
| `/trspy [on\|off]` | OP2 或 `trchat.spy` | 私聊监听 |
| `/ignore`、`/trignore <玩家> [on\|off]` | `trchat.command.ignore` | 忽略 / 取消忽略 |
| `/ignorelist` | `trchat.command.ignore` | 列出已忽略玩家 |
| `/arasple`、`/ver`、`/vers`、`/version`、`/versions`、`/help`、`/helps` | 命令控制器规则（`function.yml`） | 由 `General.Command-Controller` 规则匹配后放行 |
| 频道动态别名：`/global`、`/all`、`/shout`、`/staff`、`/message`、`/w` … | 无 | 由各频道 `Bindings.Command` 生成，重新加载时同步重建 |

> 移植版在 Mod 未定义执行体的裸节点上打印用法提示（裸 `/trchat`、`/trchat shadowmute`、
> `/trchat channel`）：Mod 由 Brigadier 报语法错误，WIT 侧没有等价报错通道。
> `/help` / `/helps` 与 Pumpkin 内置 `/help` 同名注册并由分发器合并：内置执行体被遮蔽
> （与 Mod 一致），其分页参数子节点仍可达。

## Roadmap（实验阶段后续）

- [x] 频道系统：`Bindings.Prefix` 前缀路由与 `Bindings.Command` 动态别名，对齐 Bukkit 版 `Channel` 语义
- [x] 私聊命令 `/msg` 与 `/trreply`（`PlayerCommandPreprocessEvent` 拦截）
- [x] 权限节点注册（Mod 的 `trchat.*` 节点全集，含 16 个 `trchat.color.*`）
- [x] 更新检查（GitHub release API + 语义版本比较 + 在线管理员通知，见 `updates:`）
- [x] Redis 跨服互通：监听 `trchat-message` 频道，与现有 Bukkit/Bungee/Velocity 聊天体系打通
  （自研 RESP 客户端：TCP 直连 + AUTH/SELECT + 订阅循环 + 断线重连；`BroadcastRaw` / `SendPrivateRaw` /
  `GlobalMute` / `UpdateNames` / `SendLang` 五类 action 与 `ForwardMessage` 剥壳，见 `src/redis.rs`）
- [x] 特殊字符（`special-chars.yml` 彩色 emoji 白名单 + 颜色包裹）与内置占位符
  （`%player%` / `%player_name%` / `%player_world%` / `%message%` / `%server_name%` /
  `%server_online%` / `%server_tps%` / `%server_uptime%` / `%trchat_toplayer%` 等）
- [x] 配置解析覆盖 Mod 全量 YAML：`function.yml`（命令控制器 + 内置/自定义功能）与
  `datasource.yml`（数据源，解析对齐 `PlayerDataStore` 语义，见下方说明）
- [x] 聊天过滤器（`filter.yml`：`Enable.Chat` + `Local` 敏感词 + `Ignored-Punctuations`
  跳过标点 + `WhiteList` 白名单 + `Replacement` 全角归一化），与 `settings.yml`
  `blockedWords` 双层过滤，对齐 Mod `FilterService` / `MessageGuard` 管道
- [x] 告示牌与铁砧敏感词过滤（`Enable.Sign` / `Enable.Anvil` + `Filter-Anvil-Blocked`，
  经宿主事件回写 `lines` / `rename_text`）
- [x] 云端词库（`Cloud-Thesaurus` 经 `wasi:http` 拉取、`lastUpdateDate` 去重、
  `filters/<hash>.json` 缓存兜底，每小时刷新一次、`/trchat reload` 后立即刷新，
  加载与刷新时在插件日志播报）
- [x] 配置对齐内置默认值（缺失键补全并写回文件，未知键删除，对齐 Mod `YamlConfigSynchronizer`）
- [x] 玩家数据持久化：解析 `datasource.yml`（`Type` 分支、JDBC URL、表名派生，对齐
  `PlayerDataStore` 语义，单测锁定逐字 SQL），执行层为每玩家一个 JSON 快照
  （`<存储根>/playerdata/<uuid>.json`，加入时恢复、退出与关服时落盘，原子写）

## 说明

* Pumpkin 插件 API 锁定为**最新 stable release** 标签 `0.2.0+26.3-26.51`
  （commit `204a94e`）。不跟随 master/nightly：nightly 在下一次 stable 发布前
  会有大量破坏性 WIT 变更，插件会随时无法加载。
* 事件、命令、权限等 WIT 定义位于
  `crates/pumpkin-plugin-wit/v0.1/`（[Pumpkin-MC/Pumpkin](https://github.com/Pumpkin-MC/Pumpkin) 仓库内）。
* **权限节点必须带插件命名空间**：宿主的 `register_permission` 会打回裸节点
  （`trchat.mute` → `Permission trchat.mute must use the plugin's namespace (trchat)`），
  而权限查询按完全一致的字符串匹配、未注册即拒绝。因此注册表里只有
  `trchat:trchat.mute` 这一类拼写，YAML/Mod 的裸写法（`perm "trchat.global"`、频道
  `Join-Permission`、`function.yml` 的 `Permission`）在查询前统一经 `perms::node`
  补齐命名空间。
* **插件内不要写 stderr**：宿主没有为插件接上 `wasi:cli/stderr`，`eprintln!` 的写入会
  失败并 panic，进而把整个插件 abort —— 真机冒烟测试里，注册权限失败后的那一行 stderr
  直接终止了 `on_load`。所有诊断统一走 `pumpkin:plugin/logging`（`diag::info` /
  `diag::warn`）。
* **配置必须先于命令注册装载**：命令树里的 `/global`、`/all`、`/arasple` 等别名来自
  `Bindings.Command`，而 `global_config()` 首次访问会初始化一份**默认**快照；先注册命令
  会让 `OnceLock` 被默认值占住，真实配置再也装不进去（`on_load` 已改为先
  `ChatManager::init` 再 `register_commands`，`init_global` 装不上时也会写日志）。
* **版本号**：`Cargo.toml` 只能写三段（`2.5.4+1`），对外一律报告 `mod_version`
  （`2.5.4.1`，由 `build.rs` 注入 `TRCHAT_VERSION`）——`/plugins`、`/trchat status`、
  `/ver` 与更新检查用的是同一个字符串。
* **`datasource.yml` 只做语义解析，不做真实数据库**：WASM 沙箱没有 JDBC 驱动，嵌入式
  纯 Rust 引擎（turso_core/limbo）探针失败（纯异步 + tokio，无法塞进阻塞型插件事件），
  所以 `Type` 分支与表名、SQL 文本按 `PlayerDataStore` 逐字对齐并单测锁定，但落盘走
  **每玩家一个 JSON 文件**（`<存储根>/playerdata/<uuid 去连字符>.json`，原子写；
  `SQLite.File` 的父目录决定存储根）。Mod 的 `Type: JDBC` 分支本身有缺陷（`ignoredTable` /
  `preferenceTable` 从未赋值 → NPE），移植版直接不支持，未匹配类型在加载时报错。
* 真机（Pumpkin `0.2.0+26.3-26.51`，Windows x64）冒烟测试已通过：插件加载、命令树
  （含权限拒绝路径）、`/trchat reload`、配置文件首次播种、`wasi:http` 拉取 GitHub
  release 并完成版本比较（日志 `TrChat 2.5.4.1 is up to date.`）。
