# TrChat Bukkit v2 —— 聊天主流程与频道系统规格

> 面向 Pumpkin WASM 插件（Rust）重实现的事实规格。所有结论均来自本仓库 `src/main/java` 与 `src/main/resources`。
> 行号引用格式：`文件:行范围`。未标注即为“行为描述”，不粘贴代码。

---

## 1. 聊天消息的完整处理链路

### 1.1 入口（loader 层）

| # | 步骤 | 位置 | 条件 / 结果 |
|---|------|------|-------------|
| 1 | `ServerChatEvent`（priority = HIGHEST）触发 `handleChat` | `TrChatServerEvents.java:63-69` | `service == null` 直接返回（不取消事件） |
| 2 | 世界禁用检查 `isDisabledWorld(player)` | `TrChatServerEvents.java:72-74`，实现 `TrChatCommands.java:54-76` | `chat.disabledWorlds` 为空 → 放行；否则取 `player.level().dimension()` 的完整 id（如 `minecraft:overworld`），对每条配置按 `Pattern.CASE_INSENSITIVE` + **`matches()` 全匹配** 判定。命中 → **直接 return，不取消事件**，即回落到原版聊天 |
| 3 | `event.setCanceled(true)` 后调用 `service.handleChat(player, event.getRawText())` | `TrChatServerEvents.java:75-76` | 原版广播被完全接管；无任何“失败提示” |
| 4 | Fabric 对应实现 | `TrChatServerEventsFabric.java:51-60` | 同样先判 `isDisabledWorld`（命中 → `return true` 放行原版）；否则 `handleChat(player, message.signedContent())` 后 `return false` |
| 5 | Forge 1.20.1 对应实现 | `TrChatServerEventsForge.java:63-73` | 同样先判 `isDisabledWorld`；否则 `setCanceled(true)` + `handleChat(player, event.getRawText())` |

**注意**：世界禁用发生在事件层，`ChatService` 内部**没有**世界判定，Rust 侧需在事件入口自行实现。

### 1.2 `ChatService.handleChat`（前缀路由）`ChatService.java:84-102`

1. `message = originalMessage.trim()`；为空 → 静默返回。
2. `channels.byPrefix(message)` 取前缀匹配（见 §2.1）。
3. 命中：`channel = match.channel()`，`message = message.substring(prefix.length()).stripLeading()`。
4. 未命中：`channels.byId(activeChannels.getOrDefault(uuid, "Normal"))`，失败回落 `channels.normal()`。
5. 剥离后 `message.isEmpty()` 或 `channel.options().privateChannel()` → **静默返回**（不提示）。
6. 否则 `sendPublic(player, channel, message)`。

### 1.3 `sendPublic` 步骤顺序 `ChatService.java:564-629`

| 序 | 步骤 | 位置 | 判定 | 失败提示 key | 失败结果 |
|----|------|------|------|--------------|----------|
| 1 | `canSpeak` | `ChatService.java:780-785` | `Speak-Condition` 为空 → 检查 `Join-Permission`；非空 → `ConditionEvaluator.test(condition, player)` | `Channel-No-Speak-Permission`（无参数） | return false |
| 2 | `guardMessage` | `ChatService.java:650-749` | 见 §1.4 | 见 §1.4 | return false |
| 3 | `functions.process(player, message, Disabled-Functions)` | `ChatService.java:574-576` | 返回 `ProcessedMessage(component, mentionedPlayers, crossServerSafe)`（`ChatFunctionService.java:94,722-725`） | — | — |
| 4 | `messageContext` | `ChatService.java:631-648` | `"message"` = 过滤后文本；若玩家设置了聊天颜色且（OP 或 `trchat.color.<code>`）→ 写入 `trchat_message_color` | — | — |
| 5 | `renderer.render(channel, CHAT, player, null, component, message, local)` | `ChatService.java:579-587` | 见 §3 | — | — |
| 6 | 影禁言分支 | `ChatService.java:589-601` | `moderation.shadowMuted(player)` 为真：**再渲染一次**（viewer = 自己）只发给自己 + 写日志，然后 return true | — | 不广播、不 Redis |
| 7 | Redis 转发 | `ChatService.java:603-623` | 条件：`channel.Proxy()` 且 `redis != null` 且 `crossServerSafe` | 见下 | 见下 |
| 8 | `broadcastLocal` | `ChatService.java:625,798-850` | 见 §1.5 | — | — |
| 9 | `notifyMentioned(receivers, mentioned, senderName)` | `ChatService.java:626,853-866` | 仅对**真正收到消息**且被 @ 的接收者发提及提示 | — | — |
| 10 | `logToConsole` | `ChatService.java:627,945-970` | 见 §1.6 | — | — |

Redis 分支细节（第 7 步）：
- 包体 `BroadcastRaw`，字段序：`[action, 发送者UUID, 消息JSON, Listen-Permission, Double-Transfer, Ports(";"拼接), fallback, 发送者名, 提及列表(","拼接)]`（`ChatService.java:604-614`）。
- `redis.publish` 成功 → **直接 return true，跳过本地广播**（同服玩家靠回环消息收，见 §1.7）。
- publish 失败：`Force-Proxy` 为真 → 提示 `Redis-Force-Unavailable` 并 return false；否则提示 `Redis-Fallback` 并继续本地广播。
- `crossServerSafe == false`（消息含非 `minecraft` 命名空间物品等，`ChatFunctionService.java:116-134`）→ 完全跳过 Redis。

### 1.4 `guardMessage` 守卫顺序与默认值 `ChatService.java:650-749`

按代码顺序（**不是**配置顺序）：

| 序 | 守卫 | 位置 | 触发条件 | 提示 key | 默认值 |
|----|------|------|----------|----------|--------|
| 1 | 空消息 | `:651-654` | trim 后为空 | 无（静默） | — |
| 2 | 长度限制 | `:655-658` | `message.length() > messageMaxLength` | `General-Too-Long`（参数：实际长度, 上限） | 256（`TrChatConfig.java:68`） |
| 3 | 全局禁言 | `:659-670` | `globalMute` 且**非 OP**（`hasPermissions(2)`） | `General-Global-Muting` | 默认关闭 |
| 4 | 个人禁言 | `:671-674` | `moderation.isMuted(player)` | `General-Muted`（参数：到期时间, 原因） | — |
| 5 | 防重复（相似度） | `:690-713` | `!op` 且 `antiRepeatMaxPerPeriod >= 0` 且无 `trchat.bypass.repeat` | `General-Too-Similar` | similarity 0.85、maxPerPeriod 0、period 60000ms、compareAll false |
| 6 | 防叠词 | `:715-722` | `antiDuplicatePhraseMaxRepeat > 0` 且无 `trchat.bypass.duplicate`，且 `maxConsecutiveRepeat > maxRepeat` | `General-Too-Duplicate` | maxRepeat 0（=关闭）、白名单 `哈,6,?,？,!,！` |
| 7 | 冷却 | `:724-730` | `previous != null` 且 `!op`，`cooldownMillis - (now - prev.sentAt) > 0` | `Cooldowns-Chat`（参数：剩余毫秒） | 2000ms（`TrChatConfig.java:69`） |
| 8 | 防高频 | `:732-743` | `antiHighFrequencyMaxPerPeriod > 0` 且无 `trchat.bypass.highfrequency`，窗口内条数 `>= max` | `General-Too-Frequent` | max 0（=关闭）、period 60000ms |
| 9 | 屏蔽词 | `:745-746` | 先 `filters.filterChat`（`FilterService.java:75-86`，`settings.chat()` 关闭或 OP 时跳过），再 `MessageGuard.filter(BLOCKED_WORDS, FILTER_REPLACEMENT)` | 无提示（就地替换） | blockedWords 空、replacement `*` |

守卫通过后写入 `chatStates.put(uuid, new ChatState(now, filteredMessage))`（`:747`）。**状态存的是过滤后的文本**。

关键顺序性事实：
- 全局禁言/个人禁言判定**早于**冷却与防高频。
- 冷却使用**上一次通过全部守卫的发送时间**（`ChatState.sentAt`），而守卫失败不会更新时间戳。
- OP（`hasPermissions(2)`）绕过：防重复、冷却、防高频；但**不绕过**长度、个人禁言、叠词（叠词用独立的 `trchat.bypass.duplicate`）。全局禁言仅对非 OP 生效。
- 防重复的“周期列表”**只在判定为相似时才 push**（`:705-711`）；不相似的正常消息不入表，因此 `compareAll=true` 时比较集合实际只含历史相似消息。
- 相似度比较在**过滤前**的文本上进行（第 5 步早于第 9 步）。

### 1.5 接收者计算 `broadcastLocal` `ChatService.java:798-850`

对 `server.getPlayerList().getPlayers()` 逐个判定（含发送者本人）：

1. `moderation.hasIgnored(receiver, sender.uuid)` → 跳过（忽略者收不到）。
2. `listening = channel.Always-Listen || joinedChannels[receiver].contains(channel.id().toLowerCase())`；不满足 → 跳过。
3. `canListen(receiver, channel)`：`Listen-Permission` 为空则用 `Join-Permission`；`hasPermission` 对空串返回 true（`ChatService.java:787-796`）。不满足 → 跳过。
4. 距离/范围：`Target` 以 `;` 最多切 2 段（`:807`），第二段解析为距离：
   - `SELF` → `receiver.uuid == sender.uuid`
   - `SINGLE_WORLD` / `WORLD` → 同 `level()`
   - `DISTANCE` → 同世界且 `distance >= 0` 且 `distanceToSqr <= distance²`（用平方比较，无开方）
   - 其它（含 `ALL`）→ 恒真；距离解析失败返回 -1 → `DISTANCE` 恒假
5. 命中则按接收者视角渲染（`Audience.CHAT`，viewer = receiver）并 `sendSystemMessage`，收集进 `receivers` 列表。

### 1.6 日志 `logToConsole` `ChatService.java:945-970`

- 私聊频道（`Private: true`）→ `chatLogs.logPrivate(sender, local["trchat_toplayer"], message)`；否则 `chatLogs.logNormal(sender, message)`。
- 控制台输出：`Console` 段为空 → 用 `Audience.CHAT` 渲染；非空 → `Audience.CONSOLE` 渲染。均写 `LOGGER.info`。
- 日志格式串来自配置 `logging.normalMessageFormat = "[{0}] {1}: {2}"`、`privateMessageFormat = "[{0}] {1} -> {2}: {3}"`（`TrChatConfig.java:122-123`）。
- `tick()` 每 tick 调 `chatLogs.tick()`；`tickCounter % 200 == 0` 时发布玩家名单并清理远端玩家快照（`ChatService.java:508-517`）。

### 1.7 Redis 接收端 `ChatService.java:972-1026`

- `handleRedisMessage` 先 `unwrap`：反复剥掉开头的 `"ForwardMessage"`（`:1191-1197`）。
- `BroadcastRaw`（`:996-1026`）：`data.size() < 3` 丢弃；若 `data[5]`（Ports）非空，则按 `;` 分割并要求包含本服 `SERVER_ID`，否则丢弃；反序列化 `data[2]`；对全服玩家检查 `hasPermission(player, data[3])` 且未被发送者忽略，命中即发送并计入 receivers；最后对 receivers 做提及提示。
- `SendPrivateRaw`（`:1028-1052`）：`data[1]=目标名`、`data[2]=发送者名`、`data[3]=接收者视角组件`、`data[4]=fallback`、`data[5]=可选 spy 组件`；忽略判定需能解析远端发送者 UUID；成功后写 `lastPrivateSender`。
- `GlobalMute`（`:982-986`）直接改本地 `globalMute`；`SendLang`（`:1101-1115`）按 key 直接 `Component.literal`（**未走语言文件**，除 `Function-Mention-Notify` 特殊分支）。
- 玩家名单同步：`UpdateNames`，30 秒 TTL（`REMOTE_PLAYER_TTL = 35s` 实际在 `:43`，`expireRemotePlayers` 在 `:1177-1180`）。

---

## 2. 频道系统语义

### 2.1 前缀路由 `ChannelManager.byPrefix` `ChannelManager.java:125-137`

- 遍历**所有**频道 × 所有 `Bindings.Prefix`，用 `message.startsWith(prefix)`（**区分大小写**）。
- 取**最长前缀**胜出（`Comparator.comparingInt(length)` 的 `max`）。
- 等长并列时保留遍历中**先遇到**的那个；遍历序 = `all()` 的 `Comparator.comparing(ChannelDefinition::id)`（**区分大小写的字典序**）。
- 命中后 `stripLeading()`，前缀与消息之间可含空格。

### 2.2 命令绑定 `Bindings.Command`

- `ChannelManager.byCommand(command)`：对 `all()` 顺序找**第一个** `commands` 中存在 `equalsIgnoreCase` 匹配的频道（`ChannelManager.java:119-123`）。
- 命令别名解析：`CommandInvocation.parse` 去前导空白、可选前导 `/`、按首个空白切 `alias` / `arguments`（`CommandInvocation.java:5-20`）。
- `executeBoundAlias`（`TrChatCommands.java:906-924`）：找不到 → 提示 `Channel-Command-Unbound`；非私聊频道 → `executeBoundChannel(id, arguments)`；私聊频道 → 按空白切 2 段，第二段为消息体，第一段为目标名；参数不足则走 `executeBoundChannel(id, "")`（即切换频道）。
- `routePrivateAlias`（`:926-934`）只处理 `Private: true` 的绑定，用于命令转发拦截。
- `executeChannel`（`ChatService.java:104-113`）：私聊 → `Channel-Private-Target` 且返回 0；消息空白 → `toggleChannel`；否则 `sendPublic`。
- 出厂绑定：`Global.yml` → `Prefix: ['!all']`、`Command: ['global','all','shout']`；`Staff.yml` → `Command: ['staff']`；`Private.yml` → `Command: ['msg','message','tell','talk','m','whisper','w']`；`Normal.yml` 无绑定。

### 2.3 Options 字段确切行为 `ChannelDefinition.java:19-73`

| 配置键 | 字段 | 默认 | 行为 |
|--------|------|------|------|
| `Join-Permission` | `joinPermission` | `""` | 加入与（`Listen-Permission` 为空时）接收的权限；空 = 人人可用 |
| `Listen-Permission` | `listenPermission` | `""` | 接收权限；空则继承 `Join-Permission` |
| `Speak-Condition` | `speakCondition` | `""` | 非空时替代 `Join-Permission` 做发言判定（`ConditionEvaluator`） |
| `Always-Listen` | `alwaysListen` | false | 真 → 无需加入即可收听；**退出频道时不移除 joined 记录**（`ChatService.java:464-466, 759-761`） |
| `Auto-Join` | `autoJoin` | false | 玩家无已保存成员关系时的初始频道；**全局最多一个**，且不可为私聊（`ChannelManager.java:66-89`） |
| `Private` | `privateChannel` | false | 真 → 不参与 `byPrefix` 公共路由（命中即静默丢弃）、不可加入、不可 `toggle`、`isJoinable()` 为假、使用 Sender/Receiver/Console 格式 |
| `Target` | `target` | `ALL` | 统一转大写；取值 `ALL` / `SELF` / `SINGLE_WORLD` / `WORLD` / `DISTANCE;<blocks>` |
| `Proxy` | `redis` | false | 仅表示 Bukkit 兼容 Redis 转发（注释明确“不是代理端连接”，`Example.yml:31-33`） |
| `Force-Proxy` | `forceRedis` | false | 真 → publish 失败时拒绝发送并提示 `Redis-Force-Unavailable` |
| `Double-Transfer` | `doubleTransfer` | false | 仅作为包内布尔串透传给对端（协议兼容标记） |
| `Ports` | `ports` | `[]` | 目标服务器 ID 列表；空 = 全服；接收端按 `;` 分割比对 `SERVER_ID` |
| `Disabled-Functions` | `disabledFunctions` | `[]` | 传给 `functions.process` 禁用的 function.yml 功能名（如 `Mention`） |

解析细节：`Target` 为空白 → `ALL` 并大写；`Ports`/`Disabled-Functions` 会剔除空白项、`"null"`、`"~"`；`Bindings.Prefix/Command` 同样过滤（`ChannelDefinition.java:185-190`）。

### 2.4 频道存储与成员关系

- 运行期：`ChatService` 内两张 `HashMap<UUID, …>`：`activeChannels`（当前频道 id，原样大小写）与 `joinedChannels`（`Set<String>`，**小写 id**）（`ChatService.java:58-59`）。
- 持久化：`persistChannelMembership` → `moderation.setChannels(player, active, Set.copyOf(joined))`（`:937-943`），落在 `PlayerDataStore.PlayerState`；登出时再持久化一次（`:525-535`），关服时对所有在线玩家持久化（`:552-554`）。
- 登录/重载：`restoreChannelMembership`（`:896-920`）：先剔除已不存在或不可加入的频道；集合为空 → `Auto-Join`（需有权限）否则 `Normal`；已保存的 active 必须同时“在 joined 内”且“有加入权限”，否则用 `fallbackChannel`；最终把 active 补进 joined 并持久化。
- `fallbackChannel`（`:922-935`）：若 joined 含 `"normal"` 则回 Normal；否则对 joined 排序后取第一个“可加入 + 有权限”的频道；再不行回 Normal。
- `setChannel`（`:447-458`）：非 joinable 或无 `Join-Permission` → `Channel-No-Join-Permission`（带频道 id）；成功 → 设 active、加入 joined、持久化、提示 `Channel-Join`。
- `toggleChannel`（`:751-778`）：当前已是该频道 → 退出逻辑（非 `Always-Listen` 才移除 joined），回落并提示 `Channel-Quit`；否则权限检查后加入并提示 `Channel-Join`。
- `quitChannel`（`:460-478`）：退出当前频道 → `Channel-Quit`，回落频道不同时再提示 `Channel-Join`。

### 2.5 频道加载 `ChannelManager.reload` `ChannelManager.java:41-97`

- 强制同步 4 个内置频道 `Normal/Global/Staff/Private` 与参考文件 `Example.yml`。
- 递归扫描目录下所有 `*.yml`，**排除 `Example.yml` 与旧版 `Server.yml`**，按路径排序加载；id 取自文件名去 `.yml`。
- 必须有 `normal` 键，否则 `reload()` 返回 -1（整体失败）。
- `Auto-Join: true` 多于 1 个 → 抛错返回 -1；唯一一个若为私聊 → 抛错。
- 频道 id 在 map 中统一小写；`all()` 按 `id` 字典序返回；`normal()` 即 `channels.get("normal")`；`autoJoin()` 取第一个 `Auto-Join` 频道（控制台 `sendConsole` 也用它，`ChatService.java:115-118`）。

---

## 3. ChannelRenderer 渲染算法 `ChannelRenderer.java:77-146`

1. **格式选择**：按 `Audience` 取候选列表 —— `CHAT`→`Formats`；`SENDER`→`Sender`；`RECEIVER`→`Receiver`；`CONSOLE`→`Console`（为空则回退 `Formats`）。候选已在解析期按 `priority` **降序**稳定排序，取**第一个** `condition` 通过者（`:86-95`）。
2. **无格式兜底**：所有 condition 均不通过 → 直接返回 `LegacyText.parse(resolve(message))`，`fallback` 为其 `getString()`（`:96-99`）。
3. **prefix**：按 `LinkedHashMap` 插入顺序（= YAML 书写顺序）遍历分组；每组内取第一个 condition 通过的变体（已按 priority 降序）；命中即 append。**分组名只影响输出顺序，不参与语义**（`:148-161`）。
4. **消息体**（`:104-142`）：
   - 颜色 = `local["trchat_message_color"]` 优先，否则 `msg.default-color`；去掉首字符 `&`/`§`；`ChatFormatting.getByCode(color.charAt(0))`，无效或空 → `WHITE`。
   - `cleanMessage = stripLegacyCodes(placeholders.resolve(message, subject, viewer, local))`（**消息内的 `&x` 传统颜色码被剥离**，颜色由 `default-color` 决定）。
   - `bodyText = "&" + color + cleanMessage`。
   - 特殊字符：仅当 `msg.special-char.enabled` 且 `messageComponent == null` 且 `SpecialChars.hasSpecialChars(cleanMessage)` 时，用 `wrapSpecialChars(bodyText, special-char-color, "&"+color)` 包裹（`SpecialChars.java:91-149`：连续特殊字符/ZWJ/肤色/变体选择符视为一段，段内若已出现手动颜色则不再加色，段末恢复原色）。
   - 若调用方传入了 `messageComponent`（来自 `ChatFunctionService` 的处理结果）：**不使用** `bodyText`，改为 `Component.empty()` + `applyLegacyFormat(finalFormatting)` + `append(messageComponent.copy())`；此时**不做** special-char 包裹。
   - `msg.hover` 非空 → 给整个消息体加 `HoverEvent.ShowText`（`:134-141`）。
5. **suffix**：与 prefix 同样的分组/变体规则，append 在消息体之后（`:144`）。
6. 返回 `Rendered(component, component.getString())`；`fallback` 用于 Redis 包与反序列化失败兜底（`ComponentJson.deserialize`，`ComponentJson.java:29-54`）。

### 3.1 单个部件 `component()` `ChannelRenderer.java:163-201`

1. `LegacyText.parse(placeholders.resolve(part.text, …))` 得到基础组件与样式（`LegacyText.parse` 把 `&`/`§` 码逐段转成 `Style`，`:13-33`）。
2. `hover` 非空 → `HoverEvent.ShowText`。
3. `clickEvent(part, …)` 非空 → `withClickEvent`。
4. `insertion` 非空 → `withInsertion(resolved)`。
5. `font` 非空 → `withFont(ResourceLocation.parse(resolved))`。
6. 最终 `setStyle(style)`。

### 3.2 click 生成优先级 `ChannelRenderer.java:203-274`

**固定顺序，取第一个非空**：`suggest` → `command` → `url` → `copy` → `file`；全空返回 null。

| 字段 | ClickEvent |
|------|-----------|
| `suggest` | `SuggestCommand`（补全到输入框，不发送） |
| `command` | `RunCommand`（原样执行；**代码不自动补 `/`**） |
| `url` | `OpenUrl`；先 trim，再在**第一个空格处截断**；用 `new URI(url)` 校验，抛异常则返回 null（**不校验协议**） |
| `copy` | `CopyToClipboard` |
| `file` | `OpenFile`（1.21.11 起为 `ClickEvent.OpenFile`，更早为 `Action.OPEN_FILE`） |

所有文本/属性在生成前都会做一次 `placeholders.resolve(..., subject, viewer, local)`。

---

## 4. ConditionEvaluator 表达式语义 `ConditionEvaluator.java:19-47`

按判定顺序：

| 序 | 输入 | 结果 |
|----|------|------|
| 1 | `null` / 空白 / 字面量 `~` | `true` |
| 2 | `player op` 或 `player is op`（忽略大小写，先 trim） | `player != null && hasPermissions(2)` |
| 3 | 正则 `(?:perm|permission)\s+["']?([^"'\s]+)["']?`（`CASE_INSENSITIVE`）**全串匹配**（`matches()`） | 权限节点去掉可选前导 `*` 后 `TrChatPermissions.check(player, node)`；`node` 为空 → false |
| 4 | 以 `!` 开头 | 对 `substring(1).trim()` **递归取反** |
| 5 | 其它 | `false`（**未知表达式一律不通过**） |

要点与坑：
- 只支持 3 种形态：`~`、`player op` / `player is op`、`perm "node"`（`permission` 同义），以及它们的 `!` 取反。`Example.yml:18-19` 注释里提到的 `player !op` **并不被支持**（会被判为 false）。
- `matches()` 要求**整串**符合正则，因此 `perm "a" && perm "b"` 之类的组合语法**不存在**。
- 引号可有可无（`perm node` 亦合法）；节点名不能含引号或空白。
- `player == null`（控制台）时，只有 `~`（空条件）为真。

---

## 5. MessageGuard 守卫算法 `MessageGuard.java`

| 算法 | 位置 | 语义 |
|------|------|------|
| `filter(message, blockedWords, replacement)` | `:12-21` | 对每个非空屏蔽词，做**不区分大小写**的非重叠替换；替换串 = `replacement.repeat(blocked.length())`（按**原词长度**重复，非按匹配段长度）；`blocked` 为空/空白跳过；不递归、不跨已替换区重叠匹配（`:36-47`） |
| `similarity(left, right)` | `:23-34` | 归一化（`toLowerCase(ROOT)` + `\s+` 全部删除）后：相等 → `1.0`；两串皆空 → `1.0`；否则 `1.0 - levenshtein(a,b)/max(len(a),len(b))`。Levenshtein 为滚动数组实现，替换/插入/删除代价均为 1（`:49-66`） |
| `maxConsecutiveRepeat(message, whitelist)` | `:80-119` | 返回**任意子串**的最大连续重复次数；无重复返回 1；长度 < 2 返回 1。两层剪枝：`n-i <= max` 时跳出；`(n-i)/len <= max` 时跳出内层。比较为**区分大小写**的 `regionMatches` 语义 |
| `isWhitelistedUnit(message, start, len, whitelist)` | `:126-147` | 长度 `len` 的重复单元若等于某白名单词组整数次重复，则视为白名单，跳过该 `len`（不参与 max 统计） |

默认值汇总（`TrChatConfig.java:68-106`）：

| 配置 | 默认 |
|------|------|
| `chat.messageMaxLength` | 256 |
| `chat.cooldownMillis` | 2000 |
| `chat.antiRepeatSimilarity` | 0.85 |
| `chat.antiRepeatMaxPerPeriod` | 0（相似消息**立即**拦截） |
| `chat.antiRepeatPeriodMillis` | 60000（配置为 0 时运行时按 60000 兜底，`ChatService.java:693-694`） |
| `chat.antiRepeatCompareAll` | false（只比上一条） |
| `chat.antiHighFrequencyMaxPerPeriod` | 0（关闭） |
| `chat.antiHighFrequencyPeriodMillis` | 60000（0 → 运行时 60000 兜底） |
| `chat.antiDuplicatePhraseMaxRepeat` | 0（关闭） |
| `chat.antiDuplicatePhraseWhitelist` | `哈, 6, ?, ？, !, ！` |
| `chat.blockedWords` | `[]` |
| `chat.filterReplacement` | `*` |
| `chat.disabledWorlds` | `[]`（正则，忽略大小写，全匹配） |
| `chat.globalPrefix` | `!all`（**当前代码未使用**，路由完全由频道 `Bindings.Prefix` 决定） |
| `redis.channel` | `trchat-message` |

绕过权限节点：`trchat.bypass.repeat`、`trchat.bypass.duplicate`、`trchat.bypass.highfrequency`；OP（`hasPermissions(2)`）自动绕过防重复/冷却/防高频。`trchat.color.<code>` 用于聊天颜色（`ChatService.java:289-330`）。

---

## 6. ModerationService（禁言与状态）

- 存储：`ConcurrentHashMap<UUID, PlayerState> states`（`ModerationService.java:30`），底层 `PlayerDataStore` 负责磁盘持久化。
- 载入：`playerJoined` 从 store 读入（`:45-47`）；`state(player)` 亦惰性 `computeIfAbsent` 载入（`:195-200`）。`playerLeft` 移除并 `saveAsync`（`:49-52`）；`close()` 全量保存（`:207-212`）。
- 写入：`update(state)` = 放回 map + `store.saveAsync(state)`（`:202-205`），所有 setter 走它。
- 禁言语义：
  - `muteUntil == 0` → 未禁言。
  - `muteUntil == -1` → 永久禁言。
  - `muteUntil > 0` 且 `<= now` → **查询时自动解除**（`withMute(0,"")` 并落盘）后返回 false（`:54-62`）。
  - `mute(player, durationMillis, reason)`：`durationMillis < 0` → `-1`（永久），否则 `now + duration`（`:74-77`）。
  - `muteExpiry`：`< 0` 返回字面量 `"permanent"`，否则按 `yyyy-MM-dd HH:mm:ss`（系统时区）格式化（`:64-67`）。
  - `muteReason`：空白 → `"-"`（`:69-72`）。
- 时长解析 `parseDuration`（`:148-178`）：支持 `s/m/h/d/w` 与 `permanent|forever|perm|永久`；**必须从串首到串尾连续消费**且 `total > 0`，否则返回空；溢出用 `Math.addExact/multiplyExact` 捕获后返回空。
- 其它状态：`shadowMuted`（影禁言）、`privateSpy`、`activeChannel`、`joinedChannels`、`ignoredPlayers`（UUID+名，`LinkedHashSet`）、`chatColor`。
- **检查位置**：个人禁言在 `guardMessage` 第 4 步（`ChatService.java:671-674`），即“通过长度与全局禁言检查之后、防重复/冷却/防高频之前”。`sendPrivate` 也复用 `guardMessage`（`ChatService.java:157`），所以私聊同样受禁言、长度、冷却、屏蔽词约束。`sendConsole`（`:115-149`）**不经过** `guardMessage`，因此控制台不受禁言/过滤影响。
- 影禁言（`shadowMuted`）不在守卫中拦截，而是在 `sendPublic` 第 6 步与 `sendPrivate` 中分流：只回显给发送者并写日志（`ChatService.java:589-601, 186-196`）。

---

## 7. 关键结论与实现坑（≤20 行）

1. 路由顺序固定：**世界禁用（事件层）→ 前缀/当前频道 → canSpeak → guardMessage → functions → 渲染 → 影禁言 → Redis → 本地广播 → 提及 → 日志**；任何一步失败即静默或单条提示返回。
2. 前缀匹配**区分大小写**、最长优先、等长取 `all()`（id 字典序）中最先者；频道 id 一律小写存储，active 保持原样大小写。
3. `Proxy` 就是 Redis 转发开关，与“代理端连接”无关；`Force-Proxy` 才决定 publish 失败是否拒发；`Double-Transfer` 只是协议透传标记。
4. Redis publish **成功即跳过本地广播**（依赖消息回环），Rust 侧若不做回环必须自行补本地投递，否则同服玩家收不到。
5. `guardMessage` 的顺序不能重排：全局禁言在个人禁言前，冷却用“上次成功发送时间”，防重复列表只记录相似消息。
6. 冷却/防高频周期为 0 时运行时兜底 60000ms，配置默认值本身就是 60000，`maxPerPeriod = 0` 表示“相似即拦”而非“关闭”。
7. `messageMaxLength` 按 `String.length()`（UTF-16 code unit）计，中文/emoji 计数与 Rust 的 `chars()` 不同，需显式对齐。
8. 屏蔽词替换串按**原词长度**重复；匹配为不区分大小写的非重叠扫描。
9. 消息体渲染会 `stripLegacyCodes`，玩家无法用 `&c` 染色，颜色只由 `default-color`/已选聊天颜色决定；`special-char` 仅在无组件（未走 function 处理）时才包裹。
10. `ConditionEvaluator` 只有 `~` / `player op` / `perm "node"` 及 `!` 取反；未知表达式返回 false，`player !op` 不受支持。
11. click 优先级固定 `suggest > command > url > copy > file`；`url` 只取首个空格前的片段且不校验协议。
12. `Console` 段为空时日志回落到 `Formats`；控制台消息不走 `guardMessage`。
13. 忽略（ignore）在广播、Redis 接收、私聊三处都要判；`Always-Listen` 的频道退出时不清理 joined 记录。
14. `Target` 用**平方距离**比较；`DISTANCE` 缺参数时距离为 -1 → 恒不匹配。
15. 玩家状态（active/joined/禁言/忽略/颜色）全在 `PlayerState`，登出与关服都要持久化；跨服消息只带 UUID 与名字，忽略判定依赖远端名单快照（35s TTL）。
