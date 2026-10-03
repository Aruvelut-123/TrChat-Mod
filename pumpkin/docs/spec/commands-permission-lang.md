# TrChat Bukkit v2 —— 命令 / 权限 / 语言 规格

> 面向 Pumpkin WASM 插件（Rust）重实现的事实规格。结论全部来自本仓库 `src/main/java` 与 `src/main/resources`。
> 行号引用格式：`文件:行范围`。本项目为 NeoForge/Fabric mod，**不存在 `plugin.yml`**；命令用 Brigadier 在代码内注册，权限用 NeoForge `PermissionAPI`（Fabric 用内置列表）。
> 简写：`TRC` = `src/main/java/me/arasple/mc/trchat/TrChatCommands.java`；`PERM` = `.../permission/TrChatPermissions.java`；`LANG` = `.../lang/LanguageService.java`。

---

## 1. 全部命令

### 1.1 权限判定辅助（决定“谁能用”）

| 函数 | 位置 | 语义 |
|---|---|---|
| `canUsePermission(source, node)` | `TRC:949-957` | `命令等级 2（OP）` **或** `TrChatPermissions.check(player, node)` |
| `Commands.literal(...).requires(hasPermission(2))` | `TRC:93-96,100-103` | 仅 OP 等级 2（**不看** `trchat.admin`） |
| 运行时手写检查 | `TRC:535-542` | `/trchat spy`：OP2 或 `trchat.spy`，失败 → `General-No-Permission` |
| `service == null`（未启动） | `TRC:275-277` 等 | 所有执行体统一提前 return 0；失败文本为硬编码 `&8[&3Tr&bChat&8] &cTrChat Mod is not running.`（`TRC:959-961`，**非语言键**） |

### 1.2 主命令 `/trchat`（`TRC:79-236`）

| 子命令 | 参数 | 权限（注册期） | 行为 | 失败/提示语言键 |
|---|---|---|---|---|
| `/trchat status` | — | 无（任何人/控制台） | 打印版本、频道数、默认频道、Redis/禁言/命令控制状态、在线数 | `Status-Overview`、`Status-State-*`、`Status-Creator-*`、`Status-Original-Author`、`Status-Repository-*`、`Status-Footer` |
| `/trchat status <player>` | `word` | `trchat.admin` | 目标玩家频道/延迟/禁言/Shadow/监听/OP/模式；禁言时追加到期与原因 | `Player-Status-Overview`、`Player-Status-Mute-Detail`、`Player-Status-Permanent`、`General-Player-Not-Found`、`Status-Footer` |
| `/trchat reload` | — | **OP 等级 2**（`TRC:91-97`） | 重载频道/语言/function/filter/Redis，重注册动态命令，并向所有在线玩家重发命令树 | `Reload-Success`、`Reload-Partial`、`Reload-Failed` |
| `/trchat redis reconnect` | — | **OP 等级 2**（`TRC:98-105`） | 触发 Redis 重连 | `Redis-Reconnect-Started` |
| `/trchat mute` | — | `trchat.mute` | **切换**全服禁言（取当前状态取反，`TRC:134`） | 命令本身无键；`ChatService.setGlobalMute` 向全体广播 `Global-Mute-On`/`Global-Mute-Off`（`ChatService.java:332-342`） |
| `/trchat mute on` \| `off` | — | `trchat.mute` | 显式设置全服禁言 | 同上 |
| `/trchat mute player <player> <duration> [reason]` | `word word greedy` | `trchat.mute` | 禁言在线玩家；`reason` 为空 → `-` | 成功 `Mute-Muted-Player`；时长非法 `Mute-Wrong-Format`；目标不存在 `General-Player-Not-Found` |
| `/trchat unmute <player>` | `word` | `trchat.mute` | 解除禁言 | 成功 `Mute-Cancel-Muted-Player`；`General-Player-Not-Found` |
| `/trchat shadowmute <player> [on\|off]` | `word` | `trchat.shadowmute` | 影禁言；省略 on/off → 取反 | `Mute-Shadow-On`/`Mute-Shadow-Off`；`General-Player-Not-Found` |
| `/trchat spy [on\|off]` | — | **注册期无 requires**；运行时 OP2 或 `trchat.spy`（`TRC:535-542`） | 切换私聊监听，播 `SoundEvents.ANVIL_LAND`（开启 2.0F / 关闭 0.0F） | `Private-Message-Spy-On`/`Off`（仅发给自己）；无权限 `General-No-Permission`；控制台 `General-Player-Only` |
| `/trchat msg <player> <message>` | `word greedy` | 无 | 等价 `/trmsg`（私聊） | 见 `ChatService.sendPrivate`：`Channel-No-Speak-Permission`、`General-Player-Not-Found`、`Redis-Unsafe-Item`、`Redis-Private-Unavailable` |
| `/trchat channel join <channel>` | `word` | 无 | 加入/切换频道 | `Channel-Unknown`、`Channel-No-Join-Permission`、`Channel-Join` |
| `/trchat channel join <channel> <player>` | `word word` | `trchat.command.channel.other` | 替他人切换频道 | 成功 `Channel-Join-Other`；`Channel-Not-Found`、`General-Player-Not-Found`、`General-No-Permission` |
| `/trchat channel quit` | — | 无 | 退出当前频道 | `Channel-Quit` |
| `/trchat channel quit <player>` | `word` | `trchat.command.channel.other` | 让他人退出频道 | 成功 `Channel-Quit-Other`；`General-Player-Not-Found` |
| `/trchat color <color>` | `word` | `trchat.command.color` | 设聊天颜色；`reset`/`null`/`default` 复位 | `Color-Selected`、`Color-Reset`、`Color-Invalid`、`General-No-Permission` |
| `/trchat clear <player\|*>` | `word` | `trchat.command.clear` | 向目标发 80 行空组件清屏；`*` = 全服 | 成功 `Clear-Success`；`General-Player-Not-Found` |
| `/trchat view <snapshot>` | `word` | 无 | 打开 function 背包快照 UI | 过期 `Function-Snapshot-Expired` |

> **Pumpkin deviation（§1.2，`/trchat reload` 的三态）**：Mod 的 `ReloadResult(success, channelCount, failedSections)`（`ChatService.java:348-364, 1226`）在 Rust 侧落成 `config::ReloadOutcome { channel_count, failed_sections }`，`ReloadCommand` 据此输出三个语言键，与 `TRC:430-458` 一致：`channel_count < 0` → `Reload-Failed`（参数 = `failed_sections` 逗号连接）、`failed_sections` 非空 → `Reload-Partial`（频道数 + 列表）、否则 → `Reload-Success`（频道数）。
> 段级语义：`channels` 失败 = 整次 reload 中止并保留上一份快照（对应 Mod 的 `channelCount < 0`）；`function.yml` / `filter.yml` / `lang` 失败只保留该段旧值并计入 `failedSections`。
> `settings.yml` 在 Mod 侧**根本不会重读**，但 Rust 侧快照无法在缺少它时重建，因此其失败也按整次失败上报（段名 `settings.yml`）；`datasource.yml`、`special-chars.yml` 与 Redis 在 Mod 侧同样没有失败通道（`SpecialChars.reload()` 为 `void`，`reconnectRedis()` 在本移植中是 no-op），因此不会把 reload 标记为 partial。
>
> **Pumpkin deviation（§1.2，`/trchat channel join <channel> <player>` 的未知频道键）**：Mod 的两条分支用不同键 —— 自助 `selectChannel` 用 `Channel-Unknown`（`TRC:603-608`），代他人 `setPlayerChannel` 先过 `isJoinable()`（`!privateChannel()`，`TRC:628`）再报 `Channel-Not-Found`（`TRC:631`）。Rust 侧 `ChannelJoinCommand` 已按 target 是否存在选键，并对 target 分支施加同一 `private` 过滤。
>
> **Pumpkin deviation（§1.2，`/trchat reload` 与 `/trchat redis reconnect` 的权限门）**：两者在 Mod 中都是**注册期** `.requires(hasPermission(2))`（`TRC:91-105`），即**仅 OP 等级 2**（控制台 / RCON 恒过），**不认** `trchat.admin` 节点。WIT 的 `CommandNode` 只能挂权限节点字符串、无法表达“仅 OP 等级”，因此 Rust 侧改为执行体首行的运行时判定 `sender.has_permission_level(command_wit::PermissionLevel::Two)`（`commands.rs` 的 `ReloadCommand`、`RedisReconnectCommand`）；被拒时 `reload` 打印与 Pumpkin 自身拒绝等价的 `&cYou do not have permission to use this command.`，`redis reconnect` 沿用 `General-No-Permission` 键。
>
> **Pumpkin deviation（§1.2 / §1.3，移植版补充的命令面）**：Mod 的命令树里没有以下用法，Rust 侧为可用性补上，均为**超集**、不改动 Mod 语义：
> * 裸节点用法回退——裸 `/trchat`、`/trchat shadowmute`、`/trchat channel` 在 Mod 里由 Brigadier 报“语法不完整”，WIT 反馈通道没有等价报错，故打印 `USAGE` / 频道列表文本。
>
> **Pumpkin 宿主契约（权限节点的命名空间，真机验证）**：宿主对权限节点的注册与查询有三条硬性规则，本移植的全部权限写法都由它们决定：
> * `Context::register_permission` 拒绝任何不以 `{插件名}:` 开头的节点（`plugin/api/context.rs:278-291`，报 `Permission {node} must use the plugin's namespace ({name})`），重复注册同样报错（`pumpkin-util/src/permission.rs:90-99`）；
> * `Context::register_command` 会把**不含 `:` 的** requires 自动补成 `trchat:{node}`，已含 `:` 的原样使用；
> * `PermissionManager::has_permission` 按**完全一致的字符串**查询，未注册节点一律拒绝（`pumpkin-util/src/permission.rs:330-388`）。
>
> 因此上文与下文各表“权限”列里的裸拼写（`trchat.mute`，也就是 Mod 与 YAML 的写法）在 Rust 侧统一经 `perms::node` 补齐为 `trchat:trchat.mute` 后再注册与查询，注册表里**只有**命名空间拼写。真机冒烟测试的第一版正是注册了裸拼写：宿主返回 `Err`，随后的 stderr 输出又把整个插件 abort 在 `on_load`（见 README 的“说明”一节）。
>
> 另外，`/msg`、`/tell` 在 Mod 侧来自 `Private.yml` 的 `Bindings.Command` 动态别名，Rust 侧额外**静态注册** `/msg`、`/tell`、`/trmsg`（`register_bound_aliases` 会对同名动态别名去重跳过），使私聊补全与 `/trmsg` 的词表不随频道配置变动。

### 1.3 顶层命令与别名（`TRC:238-257, 784-808`）

| 命令 | 权限 | 行为 |
|---|---|---|
| `/trmsg <player> <message>` | 无 | 与 `/trchat msg` 同一执行体（`TRC:238-245`） |
| `/trreply <message>`、别名 `/r`、`/reply` | 无 | 回复最近私聊对象（`TRC:247-251,752-756`）；无对象 → `Private-Message-No-Reply` |
| `/trmute`、`/mute` | `trchat.mute` | 与 `/trchat mute player …` 同构（无 `on/off` 分支，`TRC:860-884`） |
| `/trunmute <player>` | `trchat.mute` | 解除禁言（`TRC:787-795`） |
| `/trshadowmute`、`/shadowmute <player> [on\|off]` | `trchat.shadowmute` | 影禁言（`TRC:886-904`） |
| `/trspy` | 注册期无 requires | 切换私聊监听（`TRC:798-799`） |
| `/ignore`、`/trignore <player> [on\|off]` | `trchat.command.ignore` | 屏蔽/恢复/切换屏蔽（`TRC:810-829`）；自己 → `Ignore-Self`；结果 `Ignore-Ignored-Player`/`Ignore-Cancel-Player`；未知玩家 `General-Player-Not-Found` |
| `/ignorelist` | `trchat.command.ignore` | 列出已屏蔽玩家（`TRC:805-808`）；`Ignore-List`（空 → `-`） |

### 1.4 Command-Controller 兼容命令（`TRC:758-782, 397-428`）

`/arasple`（ABOUT）、`/ver` `/vers` `/version` `/versions`（STATUS）、`/help` `/helps`（HELP）。**注册期无权限节点**；执行时先经 `service.isCommandManaged(commandLine)`（由 `function.yml` 的 `General.Command-Controller` 匹配规则决定，`ChatFunctionService.java:188-191`），未匹配 → `Command-Controller-Disabled`。匹配成功时 ABOUT → `Command-About`、STATUS → 同 `/trchat status`、HELP → `Command-Help`。
默认规则（`src/main/resources/defaults/function.yml:17-20`）：`arasple{exact:true}{condition: perm "trchat.admin"}`、`ver(sion)?(s)?{condition: perm "trchat.admin"}`、`help(s)?{condition: perm *trchat.admin}`、`shout{cooldown: 3}`。

### 1.5 频道动态命令（`TRC:259-273, 741-750, 906-947`）

- 来源：每个频道的 `Bindings.Command` 列表（`ChannelDefinition.java:38`），**大小写不敏感去重**（`TRC:266-269`）。
- 形态：`/<alias>` 或 `/<alias> <greedy arguments>`。
- 非私聊频道：`executeBoundChannel` → `service.executeChannel`（无参数 = 切换该频道）。
- 私聊频道：首 token = 目标玩家，其余 = 消息；参数不足 → 走 `executeChannel`。
- 未绑定（reload 后残留）→ `Channel-Command-Unbound`。
- 默认绑定：`Global.yml:9-10` → `global`、`all`、`shout`；`Private.yml:8` → `msg`、`message`、`tell`、`talk`、`m`、`whisper`、`w`；`Staff.yml:7` → `staff`；`Example.yml:47` → `examplechat`。
- 另有一条事件层路由 `routePrivateAlias`（`TRC:926-934`，被 `TrChatServerEvents.java:118` 等调用）：仅处理私聊频道别名。

### 1.6 Tab 补全规则

| 位置 | 补全内容 | 位置 |
|---|---|---|
| `/trchat status <player>` | 在线玩家名 | `TRC:84-86` |
| `/trchat mute player <player>` / `/trmute` `/mute` `<player>` | 在线玩家名 | `TRC:114-116, 863-866` |
| `<duration>` | 固定字面量 `30s` `5m` `1h` `1d` `7d` `permanent` | `TRC:117-120, 867-870` |
| `/trchat unmute` `/trunmute` `/trshadowmute` `/shadowmute` `<player>` | 在线玩家名 | `TRC:138-140, 146-149, 789-792, 889-892` |
| `/trchat channel join <channel>` | `channels.all()` 中 `isJoinable()` 为真者的 `id` | `TRC:178-183` |
| `/trchat channel join <channel> <player>`、`channel quit <player>` | 在线玩家名 | `TRC:190-192, 202-204` |
| `/trchat color <color>` | `service.availableChatColors(player)` + `reset`；`service == null` 或无玩家 → 仅 `reset` | `TRC:210-214, 687-694` |
| `/trchat clear <player>` | 在线玩家名 + `*` | `TRC:221-227` |
| `/ignore` `/trignore` `<player>` | `service.knownPlayerNames()`（在线 + Redis 远端已知），`service == null` → 在线玩家名 | `TRC:813-817` |
| `/trchat msg` `/trmsg` `/trreply` `/r` `/reply`、动态别名、Controller 命令 | **无补全**（greedy 参数无 suggests） | `TRC:167-174, 238-249, 741-750, 768-782` |

---

## 2. 全部权限节点

### 2.1 NeoForge / Forge 后端（`PERM:20-119`，节点名 = `trchat.<name>`）

| 节点（逐字） | 默认 | 声明位置 |
|---|---|---|
| `trchat.global` | **对所有人开放**（`(p,u,c) -> true`） | `PERM:22-24` |
| `trchat.private` | **对所有人开放** | `PERM:25-27` |
| `trchat.admin` | OP 等级 2 | `PERM:28-39` |
| `trchat.function.mentionall` | OP 等级 2（`restricted`） | `PERM:40, 105-118` |
| `trchat.function.inventoryshow` | OP 等级 2 | `PERM:41` |
| `trchat.function.enderchestshow` | OP 等级 2 | `PERM:42` |
| `trchat.mute` | OP 等级 2 | `PERM:43` |
| `trchat.shadowmute` | OP 等级 2 | `PERM:44` |
| `trchat.spy` | OP 等级 2 | `PERM:45` |
| `trchat.command.ignore` | **对所有人开放** | `PERM:46-48` |
| `trchat.command.color` | OP 等级 2 | `PERM:49` |
| `trchat.command.clear` | OP 等级 2 | `PERM:50` |
| `trchat.command.channel.other` | OP 等级 2 | `PERM:51` |
| `trchat.bypass.cmdcooldown` | OP 等级 2 | `PERM:52` |
| `trchat.bypass.repeat` | OP 等级 2 | `PERM:53` |
| `trchat.bypass.duplicate` | OP 等级 2 | `PERM:54` |
| `trchat.bypass.highfrequency` | OP 等级 2 | `PERM:55` |
| `trchat.color.0` … `trchat.color.f`（16 个，字符取自 `"0123456789abcdef"`） | OP 等级 2 | `PERM:56-69` |

注册：`PERM:74-81`（`register` 收集 17 个 + 16 个颜色节点）。默认值在 `>=1.21.11` 用 `PermissionLevel.byId(2)`，否则 `player.hasPermissions(2)`。

### 2.2 Fabric 后端（`PERM:120-167`）

无权限 API，使用内置判定：
- 恒开放（`OPEN_NODES`，`PERM:131-135`）：`trchat.global`、`trchat.private`、`trchat.command.ignore`。
- 前缀恒开放（`OPEN_PREFIXES`，`PERM:137-139`）：`trchat.color.`（即全部 16 个颜色节点）。
- 其余一切（含未知节点）→ OP 等级 2（`PERM:157-165`）。

### 2.3 `check()` 语义

| 后端 | 行为 | 位置 |
|---|---|---|
| NeoForge/Forge | `permission` 为 null/空白 → `true`；否则在 `PermissionAPI.getRegisteredNodes()` 中**大小写不敏感**匹配 `getNodeName()` 且类型为 `BOOLEAN` → 取该节点值；未注册 → OP 等级 2 | `PERM:83-103` |
| Fabric | null/空白 → `true`；开放节点/前缀 → `true`；否则 OP 等级 2 | `PERM:144-166` |

### 2.4 每个节点的检查点

| 节点 | 检查位置 |
|---|---|
| `trchat.admin` | `TRC:83`（`/trchat status <player>`）；`UpdateChecker.java:76`（更新通知）；`function.yml:17-19` 的 `perm "trchat.admin"` 条件 |
| `trchat.mute` | `TRC:107, 136, 788, 862` |
| `trchat.shadowmute` | `TRC:145, 888` |
| `trchat.spy` | `TRC:536, 538` |
| `trchat.command.ignore` | `TRC:806, 812` |
| `trchat.command.color` | `TRC:209` |
| `trchat.command.clear` | `TRC:219` |
| `trchat.command.channel.other` | `TRC:189, 201` |
| `trchat.color.<0-9a-f>` | `ChatService.java:293-299`（`availableChatColors`）、`315-322`（`setChatColor`）、`637-643`（消息上下文着色） |
| `trchat.bypass.cmdcooldown` | `ChatFunctionService.java:180` |
| `trchat.bypass.repeat` | `ChatService.java:692` |
| `trchat.bypass.duplicate` | `ChatService.java:716` |
| `trchat.bypass.highfrequency` | `ChatService.java:733` |
| `trchat.global` | `Global.yml:2` 的 `Speak-Condition: 'perm "trchat.global"'`，经 `ConditionEvaluator.java:35-42` |
| `trchat.private` | `Private.yml:2` 的 `Join-Permission`，经 `ChatService.java:448, 468, 769, 783, 904, 912, 931` |
| `trchat.function.*` | `function.yml:31,47,53` 的 `Permission:`，经 `ChatFunctionService.java:286-288` |
| 任意节点（动态） | `ChatService.java:794-795` 的 `hasPermission` 包装；`ConditionEvaluator.java:41`（`perm`/`permission` 语法，`*` 前缀被剥离）；`PlaceholderResolver.java:190`（`player_has_permission_<node>`）；`FilterService.java:77-83, 95-101` 使用 OP2 绕过（**不是**节点） |

---

## 3. 语言系统

### 3.1 文件格式与加载

| 项 | 事实 | 位置 |
|---|---|---|
| 运行时目录 | `Platform.configDir()/trchat/lang` | `LANG:34-39` |
| 内置默认 | `src/main/resources/defaults/lang/{zh_CN,en_US,es_ES}.yml`（`DEFAULTS` 数组顺序） | `LANG:24, 172-180` |
| 加载顺序 | 先同步 3 个默认语言，再遍历目录内所有 `*.yml` | `LANG:50-70` |
| 格式 | 顶层扁平 YAML 映射；键用 `-` 连接（如 `General-Player-Not-Found`），**不是点号**；唯一嵌套映射为 `Placeholder-Translations` | `zh_CN.yml:1-142` |
| 值类型 | 单引号 legacy 字符串（含 `&` 颜色码）或 `|-` 块标量多行文本 | `zh_CN.yml:26-32, 41-50` |
| 同步/修复 | `YamlConfigSynchronizer.synchronize(file, "/defaults/lang/<lang>.yml", Set.of())`；文件缺失则从资源复制；**缺失键补默认值，未知键被删除**（空 openMapPaths，`Placeholder-Translations` 内层同样不开放） | `LANG:172-180`；`YamlConfigSynchronizer.java:34-73, 101-138` |
| 扁平化 | 顶层键原样保留；`Placeholder-Translations` 展开为 `Placeholder-Translations.<english 小写>` | `LANG:146-158` |
| 值扁平化 | 字符串直取；列表取首个非空字符串或含 `text` 的映射；映射取 `text`；null → `""` | `LANG:160-170` |

### 3.2 键查找、回退与参数替换

| 项 | 事实 | 位置 |
|---|---|---|
| 查找 | `Map.get(key)`，**大小写敏感、精确匹配** | `LANG:76-85` |
| 回退顺序 | ① 玩家语言 → ② 默认语言（`trchat.lang.defaultLanguage`）→ ③ `en_us` → ④ **原样返回键名本身** | `LANG:77-80, 133-138` |
| 参数替换 | 顺序替换 `{0}`、`{1}`… 为 `String.valueOf(arguments[i])`；仅按位置，无命名占位符 | `LANG:81-83` |
| 渲染 | `component()` = `LegacyText.parse(text(...))`（解析 `&` 颜色/格式码） | `LANG:72-74` |
| 玩家语言 | 玩家：`player.clientInformation().language()`（`>=1.20.5`）/ `player.getLanguage()`；null（控制台）→ 配置默认语言 | `LANG:118-127` |
| 键规范化 | `trim()` + `-`→`_` + 小写（`zh-CN` 与 `zh_CN` 等价） | `LANG:140-144` |
| 默认语言配置 | `defaultLanguage`，默认值 `zh_CN` | `TrChatConfig.java:64, 225, 340` |
| 占位符本地化 | 仅当 `PlaceholderCatalog.isLocalizable(token)` 为真 **且** 结果匹配 `[A-Za-z]+(?:[ _-][A-Za-z]+)*`（纯英文词）；键 = `Placeholder-Translations.<value 小写>` | `LANG:26-28, 100-116`；`PlaceholderCatalog.java:42-61, 86-97` |
| 可本地化 token 白名单 | 固定 18 个（`server_has_whitelist`、`player_gamemode`、`player_is_op`、`player_direction`、`player_world_type` 等，`PlaceholderCatalog.java:43-60`）+ 前缀 `server_countdown_`、`player_has_permission_`、`player_has_potioneffect_` | `PlaceholderCatalog.java:42-61, 86-97` |

### 3.3 语言文件键清单结构（三语文件键集**完全一致**）

`zh_CN.yml` / `en_US.yml` / `es_ES.yml` 各 145 行、**95 个顶层键** + `Placeholder-Translations` 内 **19 条**。

键集与本地重实现的 `src/main/resources/defaults/lang/` 逐键一致（92 键），另按上游 Mod 的语言表补齐云端
词库播报所需的 3 个 `Plugin-*` 键（上游还有 `Plugin-Loaded-Channels`、`Plugin-Reloaded`、
`Plugin-Proxy-*` 等键，本地重实现未搬，端口同样未加）。

| 前缀组 | 数量 | 代表性键名 |
|---|---|---|
| `Function-*` | 14 | `Function-Snapshot-Expired`、`Function-Mention-Notify`、`Function-Mention-Title`、`Function-Mention-Subtitle`、`Function-Mention-Hover`、`Function-Mention-All-Hover`、`Function-Item-Air`、`Function-Item-Title`、`Function-Inventory-Format/Hover/Title`、`Function-EnderChest-Format/Hover/Title` |
| `Status-*` | 13 | `Status-Overview`、`Status-State-Enabled/Disabled/Connected/Reconnecting`、`Status-Creator-Prefix/Link/Link-Hover`、`Status-Original-Author`、`Status-Repository-Prefix/Link/Link-Hover`、`Status-Footer` |
| `Channel-*` | 11 | `Channel-Join`、`Channel-Quit`、`Channel-Join-Other`、`Channel-Quit-Other`、`Channel-Unknown`、`Channel-Not-Found`、`Channel-No-Speak-Permission`、`Channel-No-Join-Permission`、`Channel-Auto-Join-Missing`、`Channel-Command-Unbound`、`Channel-Private-Target` |
| `General-*` | 9 | `General-No-Permission`、`General-Player-Not-Found`、`General-Player-Only`、`General-Muted`、`General-Global-Muting`、`General-Too-Long/Too-Similar/Too-Duplicate/Too-Frequent` |
| `Updater-*` | 6 | `Updater-Available`、`Updater-Link`、`Updater-Link-Prefix`、`Updater-Link-Hover`、`Updater-Changelog`、`Updater-Changelog-Empty` |
| `Mute-*` | 5 | `Mute-Muted-Player`、`Mute-Cancel-Muted-Player`、`Mute-Shadow-On/Off`、`Mute-Wrong-Format` |
| `Command-*` | 5 | `Command-About`、`Command-Help`、`Command-Controller-Disabled`、`Command-Controller-Deny`、`Command-Controller-Cooldown` |
| `Redis-*` | 5 | `Redis-Reconnect-Started`、`Redis-Private-Unavailable`、`Redis-Unsafe-Item`、`Redis-Force-Unavailable`、`Redis-Fallback` |
| `Private-*` | 4 | `Private-Message-No-Reply`、`Private-Message-Spy-Format`、`Private-Message-Spy-On/Off` |
| `Ignore-*` | 4 | `Ignore-Ignored-Player`、`Ignore-Cancel-Player`、`Ignore-List`、`Ignore-Self` |
| `Color-*` | 3 | `Color-Selected`、`Color-Reset`、`Color-Invalid` |
| `Player-Status-*` | 3 | `Player-Status-Overview`、`Player-Status-Mute-Detail`、`Player-Status-Permanent` |
| `Reload-*` | 3 | `Reload-Success`、`Reload-Partial`、`Reload-Failed` |
| `Global-*` | 2 | `Global-Mute-On`、`Global-Mute-Off` |
| 单键 | 5 | `Console-Name`、`Clear-Success`、`Cooldowns-Chat`、`Filter-Anvil-Blocked`、`Placeholder-Translations`（映射，19 条：`yes`/`no`、4 个游戏模式、`Overworld`/`Nether`/`The End`、8 个方位 `N`…`NW`、`invalid date`、`invalid format and time`） |
| `Plugin-*` | 3 | `Plugin-Loaded-Filter-Local`、`Plugin-Loaded-Filter-Cloud`、`Plugin-Failed-Load-Filter-Cloud`（移植补齐：云端词库刷新在插件日志里播报，见 `docs/spec/placeholder-function-filter.md` §3.4） |

> **Pumpkin deviation（§3.3，默认文件里存在但 Rust 侧无引用点的键）**：三份默认语言文件保留了完整的 Mod 键集，下列键暂未被引用，因为对应子系统不在本次移植范围：
> `Console-Name`（Mod 用作 console 日志前缀/查看者名，Rust 侧 console 视图另行拼装）。
> `Filter-Anvil-Blocked`（告示牌/铁砧过滤）、`Status-State-Connected` / `Status-State-Reconnecting`（Redis 已接入，`/trchat status` 按连接状态报三态）、`Status-Creator-Link-Hover` / `Status-Repository-Link-Hover`（反馈组件的 hover）、`Updater-*`（更新检查器）均已接入。
> 其余“只在 Mod 源码出现”的字符串 —— `TrChat-Data-Save`、`TrChat-Filter-Cloud`、`trchat-message`、`trchat-neoforge`、`User-Agent` —— 分别是线程名、虚拟线程名、配置频道名、旧版数据目录名与 HTTP 头，**不是语言键**。

---

## 4. 关键结论与实现坑（≤15 行）

1. `/trchat reload` 与 `/trchat redis reconnect` 只认 **OP 等级 2**，**不认** `trchat.admin`；`trchat.admin` 仅用于 `status <player>`、更新通知、`function.yml` 的 `perm "trchat.admin"`。
2. `canUsePermission` 语义是 **OP2 或节点**，且所有管理节点的默认值本身就是 OP2 → 无权限 mod 时权限节点形同 OP；Rust 侧必须把“节点默认值”和“检查逻辑”分开建模。
3. Fabric 后端把 `trchat.global`、`trchat.private`、`trchat.command.ignore`、`trchat.color.*` 视为**恒真**；其余含未知节点一律 OP2。
4. `/trchat spy`、`/trspy` **注册期无权限**，运行时才查 OP2/`trchat.spy`；`/trchat msg`、`/trmsg`、`/trreply`、`/r`、`/reply`、`/trchat view`、`/trchat status`、`/trchat channel join|quit`（无 target）对所有人开放。
5. `/trchat mute`（无参数）是**取反切换**，不是开启；反馈只来自 `setGlobalMute` 的全服广播，命令自身不返回提示键。
6. `service == null` 的提示是**硬编码英文** `TrChat Mod is not running.`，不在语言文件里，Rust 侧需自带或忽略。
7. 语言键是**大小写敏感的 `-` 连字键**（非点号），缺失键会**把键名原样显示给玩家**，不会崩溃也不为空 —— 建议 Rust 侧记录告警。
8. 回退链固定为 玩家语言 → 默认语言 → `en_us` → 键名；语言目录名 `-`→`_` 并小写，所以 `zh-CN` 与 `zh_CN` 命中同一文件。
9. 参数替换是纯位置式的 `{0}`/`{1}`，且 `Placeholder-Translations` 的键在查找时被**小写化**。
10. 语言 YAML 同步会**删除未知键**（含 `Placeholder-Translations` 内的未知英文条目），只有默认文件里存在的键会被保留或补齐。
11. `Placeholder-Translations` 只对白名单 token **且结果为纯英文**时生效；数字、混合文本、非白名单 token 一律原样返回。
12. 三语文件键集完全一致（92 顶层 + 19 占位符）；新增键必须同时改三份默认文件，否则会被同步机制删除。
13. 频道动态命令来自 `Bindings.Command`，大小写不敏感去重，reload 后需重注册并重发命令树；未绑定残留返回 `Channel-Command-Unbound`。
14. Tab 补全中 `/ignore` 用 `knownPlayerNames()`（含 Redis 远端玩家），其余用在线玩家名；`/trchat color` 的候选依赖 `trchat.color.*` 权限，无玩家时仅 `reset`。
15. Controller 命令会**遮蔽原版 `/help`**，且默认仅在 `General.Command-Controller.Enabled: true` 且命中规则时可用，否则一律 `Command-Controller-Disabled`。
