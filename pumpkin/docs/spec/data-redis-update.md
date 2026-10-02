# TrChat Bukkit v2 规格：玩家数据 / 聊天日志 / Redis 跨服 / 协议 / 更新检查 / 启动流程

> 面向 Pumpkin WASM 插件（Rust）重实现的事实规格。来源：本仓库 `src/main/java` 与 `src/main/resources`。
> 行号引用格式：`文件:行范围`。分支基线：`upstream-sync.properties` 记录 `upstream_commit=53f18abd3a7579f20333d02a957ad155fe81bedb`，本次核对 `upstream/v2` 无新提交。
> 本项目为 NeoForge/Fabric **模组**（非 Bukkit 插件），无 `plugin.yml`/`bungee.yml`/`velocity-plugin.json`；元数据在 `src/main/templates/META-INF/{neoforge.mods.toml,mods.toml}`、`src/main/templates/fabric.mod.json`。

---

## 1. 玩家数据存储（`data/PlayerDataStore.java`）

### 1.1 后端与表名

| Type 取值 | JDBC URL 模板 | 驱动类 | 默认端口 | 来源 |
|---|---|---|---|---|
| `SQLite` / `Local` | `jdbc:sqlite:<绝对路径>` | `org.sqlite.JDBC` | — | `PlayerDataStore.java:65-77` |
| `MySQL` | `jdbc:mysql://<host>:<port>/<db>[?<Parameters>]` | `com.mysql.cj.jdbc.Driver` | 3306 | `:78-79`, `:135-158` |
| `MariaDB` | `jdbc:mariadb://…` | `org.mariadb.jdbc.Driver` | 3306 | `:80-81` |
| `PostgreSQL` / `Postgres` | `jdbc:postgresql://…` | `org.postgresql.Driver` | 5432 | `:82-83` |
| `JDBC`（自定义） | `JDBC.Url` 原样 | `JDBC.Driver`（可空，空则跳过 `Class.forName`） | — | `:84-93`, `:94-95` |

- 配置文件：`config/trchat/datasource.yml`，由 `YamlConfigSynchronizer.synchronize(config, "/defaults/datasource.yml", Set.of())` 与内嵌默认同步（`:58-63`）。
- SQLite `File` 默认 `data.db`，相对 `config/trchat`，也接受绝对路径（`:67-70`；`defaults/datasource.yml:9-12`）。
- 表名（SQLite 分支）：`trchat_player_state`、`trchat_player_channels`、`trchat_player_ignored`、`trchat_player_preferences`（`:73-76`）。
- 表名（网络库分支）：`<Table-Prefix>player_state` / `player_channels` / `player_ignored` / `player_preferences`，`Table-Prefix` 默认 `trchat_`（`:152-156`；`defaults/datasource.yml:59`）。
- `safeIdentifier()` 要求前缀匹配 `[A-Za-z0-9_]+`，否则抛 `IllegalArgumentException("Invalid table prefix")`（`:361-364`）。
- `initialize()` 抛 `IllegalStateException("Unable to initialize player data storage")` 时构造失败（`:130-132`）。

### 1.2 表结构与列定义（`CREATE TABLE IF NOT EXISTS`，`:97-128`）

| 表 | 列 | 类型 / 约束 |
|---|---|---|
| `trchat_player_state` | `uuid` | `VARCHAR(36) PRIMARY KEY` |
| | `player_name` | `VARCHAR(64) NOT NULL` |
| | `mute_until` | `BIGINT NOT NULL DEFAULT 0` |
| | `mute_reason` | `VARCHAR(512) NOT NULL DEFAULT ''` |
| | `shadow_muted` | `INTEGER NOT NULL DEFAULT 0` |
| | `private_spy` | `INTEGER NOT NULL DEFAULT 0` |
| `trchat_player_channels` | `uuid` / `channel_id` / `is_active` | `VARCHAR(36) NOT NULL` / `VARCHAR(128) NOT NULL` / `INTEGER NOT NULL DEFAULT 0`；`PRIMARY KEY (uuid, channel_id)` |
| `trchat_player_ignored` | `uuid` / `ignored_uuid` / `ignored_name` | `VARCHAR(36)` / `VARCHAR(36)` / `VARCHAR(64)`，均 `NOT NULL`；`PRIMARY KEY (uuid, ignored_uuid)` |
| `trchat_player_preferences` | `uuid` / `chat_color` | `VARCHAR(36) PRIMARY KEY` / `VARCHAR(32) NOT NULL DEFAULT ''` |

### 1.3 持久化字段清单（`PlayerState`，`:374-453`）

| 字段 | 类型 | 语义 / 规范化 |
|---|---|---|
| `uuid` | `UUID` | 主键，存为 36 位带连字符字符串 |
| `playerName` | `String` | 每次 `save` 覆盖写入最新名字（`:202`, `:211`） |
| `muteUntil` | `long` | `0`=未禁言；`<0`=永久；`>0`=epoch millis（`ModerationService.java:56-61,74-77`） |
| `muteReason` | `String` | 空串为默认 |
| `shadowMuted` | `boolean` | 存为 0/1 |
| `privateSpy` | `boolean` | 存为 0/1 |
| `activeChannel` | `String` | `trim().toLowerCase(Locale.ROOT)`（`:445-447`） |
| `joinedChannels` | `Set<String>` | 逐项小写化、去空；**若 activeChannel 非空且不在集合内则自动补入**（`:394-398`） |
| `ignoredPlayers` | `Set<IgnoredPlayer>` | `IgnoredPlayer(uuid, name)`；name 为空/空白时回落为 uuid 字符串（`:455-462`） |
| `chatColor` | `String` | 小写后必须匹配 `[0-9a-f]`，否则归一为 `""`（`:449-452`） |

`PlayerState.empty(uuid,name)` = `(uuid, name, 0L, "", false, false, "", Set.of(), Set.of(), "")`（`:403-405`）。

### 1.4 读写时机与 SQL 序列

| 时机 | 行为 | 来源 |
|---|---|---|
| 玩家加入 | `ModerationService.playerJoined` → `store.load(uuid, name)` | `ModerationService.java:45-47` |
| 首次/缺失记录 | `load` 未命中 → 返回 `empty` 并**立即同步 `save(state)` 落库** | `PlayerDataStore.java:184-186` |
| `load` SQLException | 记 ERROR，返回 `empty` 并 `save`（可能再次失败） | `:181-186` |
| 懒加载 | `state(player)` 用 `computeIfAbsent` → `store.load` | `ModerationService.java:195-200` |
| 任意状态变更 | `update(state)` → `states.put` + `store.saveAsync(state)` | `ModerationService.java:202-205` |
| 玩家退出 | `states.remove` + `store.saveAsync(state)` | `ModerationService.java:49-52` |
| 关闭 | `store.close()`；随后对 `states.values()` 逐个**同步** `store.save`，再 `states.clear()` | `ModerationService.java:207-212` |

- `load` SQL：`SELECT mute_until,mute_reason,shadow_muted,private_spy FROM <state> WHERE uuid=?`（`:161`），命中后再查 `loadMembership`（`:233-253`）、`loadIgnoredPlayers`（`:276-292`）、`loadChatColor`（`:315-323`）。
- `save`（`synchronized`，`:193-231`）：`setAutoCommit(false)` → 先 `UPDATE <state> SET player_name=?,mute_until=?,mute_reason=?,shadow_muted=?,private_spy=? WHERE uuid=?`；`executeUpdate()==0` 时改走 `INSERT`（`:194-218`）→ `saveMembership` → `saveIgnoredPlayers` → `savePreferences` → `commit`；异常则 `rollback` 并记 ERROR（不抛出）。
- `saveMembership`（`:255-274`）：先 `DELETE FROM <channels> WHERE uuid=?`，再按 `joinedChannels` **排序**后批量 `INSERT`，`is_active = channel.equalsIgnoreCase(activeChannel) ? 1 : 0`；集合为空时直接 return（仅删除）。
- `saveIgnoredPlayers`（`:294-313`）：先 DELETE 再批量 INSERT，空集合直接 return。
- `savePreferences`（`:325-340`）：先 UPDATE，`executeUpdate()==0` 时 INSERT。
- `connection()`：`user` 为空白 → `DriverManager.getConnection(url)`，否则带 user/password（`:342-346`）。**每次调用新建连接，无连接池。**

### 1.5 缓存与异步策略

- **无独立缓存层**；缓存即 `ModerationService.states`（`ConcurrentHashMap<UUID, PlayerState>`，`ModerationService.java:30`）。
- 写路径异步：`saveExecutor` 单线程，`>=1.20.5` 用虚拟线程工厂命名 `TrChat-Data-Save-`，否则守护线程 `TrChat-Data-Save`（`PlayerDataStore.java:44-54`）。
- 关闭：`saveExecutor.shutdown()` + `awaitTermination(10, SECONDS)`，超时记 WARNING（`:348-359`）。
- 读路径同步（在事件线程 / 计算线程上阻塞 JDBC）。

### 1.6 WASM 移植执行层（Rust）

- **配置解析**：`crate::datasource::Datasource::resolve` 复刻 §1.1 全部行为 —— `Type` 分支（`sqlite`/`local`、`mysql`、`mariadb`、`postgresql`/`postgres`）、JDBC URL 组合（`scheme://host:port/database?parameters`，空值回退 `127.0.0.1` / 默认端口 / `trchat`）、网络分支表名前缀 `safeIdentifier(prefix + 表名)`（默认 `trchat_`；非法字符 panic，对应 Mod `IllegalArgumentException`）。
- **SQL 文本**：`datasource::Datasource::{ddl,load_*,save_*}` 生成 §1.2-§1.4 的 **逐字** SQL（`CREATE TABLE`、`SELECT`、`UPDATE`、`INSERT`、`DELETE`；save 先 UPDATE、`executeUpdate()==0` 才 INSERT 的双语句序列由调用方按 §1.4 顺序执行）。单测锁定逐字文本。
- **执行层（偏差）**：WASM 沙箱无 JDBC 驱动、`turso_core` 嵌入式引擎探针失败（§7），故 `crate::playerdata::PlayerStore` 以 **每玩家一个 JSON 文件**（`<存储根>/playerdata/<uuid 去连字符>.json`，原子写 = 临时文件 + rename）承载与 §1.3 相同的字段集合。`datasource.yml` 的 `SQLite.File` 决定**存储根**：解析后绝对 DB 路径的父目录（`folder.resolve(configured).normalize()` 后取 parent；空值 / 网络后端回退插件数据文件夹），不承载真实 DB 文件。
- **读写时机（偏差）**：Mod 在任意状态变更后即 `saveAsync`（§1.4）；本 port 仅在 `player-join`（`load` → 安装会话）、`player-leave`（`save`）与 `on_unload`（`flush_all`，对应 `store.close()` 的逐个同步 `save`）落盘。会话期间的变更只存在于内存态（与 Redis 中继共享），崩溃/强杀最多丢**当前会话**的增量 —— 与 Mod「退出必存 + 关闭全存」的可恢复面等价。
- **默认频道**：`player-join` 以 `config.default_channel()` 为兜底（§1.4 的 `join`），持久化 `activeChannel` 优先于配置默认，与 Mod 恢复 `is_active=1` 行一致。

---

## 2. 聊天日志 `ChatLogService`（`data/ChatLogService.java`）

| 项目 | 事实 | 来源 |
|---|---|---|
| 存储位置 | `Platform.configDir()/trchat/logs` | `:30-32` |
| 后端 | **纯文本文件**，与玩家数据库无关（不走 JDBC） | 全文件 |
| 文件命名 | `yyyy-MM-dd.txt`（`DateTimeFormatter.ISO_LOCAL_DATE`），按天分文件 | `:21`, `:73` |
| 每行前缀 | `HH:mm:ss`（`LINE_TIME`，系统时区） | `:22`, `:109-111` |
| 编码 | UTF-8，`CREATE` + `APPEND` | `:72-78` |
| 换行 | `System.lineSeparator()` | `:70` |

- 记录字段（模板占位符按序号替换 `{0}`…`{n}`，`format()` 见 `:101-107`）：
  - 普通聊天：`LOG_NORMAL_FORMAT`，默认 `[{0}] {1}: {2}` = `[时间] 发送者: 消息`（`:39-43`；`TrChatConfig.java:122`）。
  - 私聊：`LOG_PRIVATE_FORMAT`，默认 `[{0}] {1} -> {2}: {3}` = `[时间] 发送者 -> 目标: 消息`（`:45-49`；`TrChatConfig.java:123`）。
- 内容清洗：`safe()` 把 `\r`、`\n` 替换为空格，`null` → 空串（`:113-115`）——**单行约束**。
- 缓冲：`ConcurrentLinkedQueue<String> pending`，写入只入队，不落盘（`:27`, `:39-49`）。
- 落盘时机：`tick()` 每 6000 ticks（`20*60*5` = 5 分钟）`flush()`；`close()` 也会 `flush()`（`:23`, `:51-59`, `:117-120`）。
- 轮转/保留：`deleteExpired()` 在构造时执行一次，并在每 72000 ticks（`20*60*60` = 1 小时）执行（`:24`, `:36`, `:56-58`）。`retentionDays <= 0` 时**禁用删除**（默认 `0`）；否则删除 `lastModifiedTime` 早于 `now - days*86400s` 的普通文件（`:84-99`；`TrChatConfig.java:124-126`）。
- 查询方式：**无任何查询 API**，只能离线读文件。
- 调用点：`ChatService.logToConsole` 中按 `channel.options().privateChannel()` 分流到 `logPrivate` / `logNormal`（`ChatService.java:951-959`）；控制台消息也写普通日志（`ChatService.java:137,146`）。

---

## 3. Redis 跨服

### 3.1 连接参数（`redis/RedisSettings.java` + `TrChatConfig.java`）

| 字段 | 默认值 | 范围 | 配置键 | 来源 |
|---|---|---|---|---|
| `enabled` | `false` | — | `redis.enabled` | `TrChatConfig.java:145` |
| `host` | `"127.0.0.1"` | — | `redis.host` | `:146` |
| `port` | `6379` | 1..65535 | `redis.port` | `:147` |
| `username` | `""` | — | `redis.username` | `:148-150` |
| `password` | `""` | — | `redis.password` | `:151-153` |
| `database` | `0` | 0..15 | `redis.database` | `:154` |
| `connectTimeoutMillis` | `3000` | 100..60000 | `redis.connectTimeoutMillis` | `:155` |
| `socketTimeoutMillis` | `0` | 0..600000 | `redis.socketTimeoutMillis` | `:156` |
| `reconnectDelayMillis` | `3000` | 100..60000 | `redis.reconnectDelayMillis` | `:157` |
| `channel` | `"trchat-message"` | — | `redis.channel` | `:158-160` |

`RedisSettings.fromConfig()` 按上表顺序构造 record（`RedisSettings.java:17-29`）。

### 3.2 RESP 客户端（`redis/RespConnection.java`）

- 裸 `java.net.Socket`：`connect(host, port, connectTimeoutMillis)`；`setKeepAlive(true)`、`setTcpNoDelay(true)`、`setSoTimeout(socketTimeoutMillis)`；`BufferedInputStream`/`BufferedOutputStream`（`:22-29`）。
- 握手：`password` 非空白时 `AUTH <password>`（`username` 空白）或 `AUTH <username> <password>`，响应必须 `OK`（`:31-36`, `:120-124`）；`database != 0` 时 `SELECT <db>`（`:37-39`）。
- 请求编码：`*<参数个数>\r\n`，每个参数 `$<UTF-8 字节数>\r\n<bytes>\r\n`（`:47-57`）。
- 响应解析 marker：`+` 简单字符串、`-` → `IOException("Redis error: …")`、`:` → `Long`、`$` bulk string（`-1` → `null`）、`*` 数组（`-1` → `null`）；其他 marker 抛异常（`:59-72`）。
- `command()` 为 `synchronized` write+read（`:42-45`）；`read()` 本身非同步，仅供订阅线程独占使用。

### 3.3 发送 / 接收流程（`redis/RedisBridge.java`）

- `start()`：`running` CAS false→true；启动守护平台线程 `"TrChat Redis subscriber"` 跑 `subscriptionLoop`（`:27-41`）。
- `publish(TrChatMessage)`（`:43-61`）：未运行返回 `false`；`RedisEnvelopeCodec.encode`；`synchronized(this)` 内惰性建 `publisher`，执行 `PUBLISH <channel> <payload>`，返回 `response instanceof Long && >0`（订阅者数量 > 0 才算成功）。`IOException` → `closePublisher()` + WARN `"Redis publish failed: {}"` + `false`。
- `subscriptionLoop()`（`:80-120`）：外层 `while (running)` → 新建连接 → `write("SUBSCRIBE", channel)` → 读确认（必须是 `List`）→ `subscribed = true` → INFO `"Connected to Redis at {}:{} on channel '{}'"` → 内层 `while (running)` 读消息；仅接受 `List`、`size>=3`、`values[0]=="message"`、`values[1]==channel`、`values[2] instanceof String` → `receive(payload)`。异常时 WARN `"Redis subscription lost: {}"`；`finally` 置 `subscribed=false` 并关闭订阅连接。
- `receive(payload)`（`:122-128`）：`decode` → `receiver.accept`；`RuntimeException` → WARN `"Ignoring malformed TrChat Redis message: {}"`。
- 订阅回调在 `ChatService` 里被重新投递到主线程：`msg -> server.execute(() -> handleRedisMessage(msg))`（`ChatService.java:494-495`）。

### 3.4 失败重连

- 订阅：外层 `while (running)` 无限重试；每次失败后若仍在运行则 `Thread.sleep(settings.reconnectDelayMillis())`（默认 3000ms，固定间隔，无退避）；中断时恢复中断标志（`:103-119`）。
- 发布：**无重连循环**；连接失效时置 `publisher = null`，下次 `publish` 惰性重建（`:55-59`, `:130-133`）。
- `close()`：`running` CAS true→false，`subscribed=false`，关订阅与发布连接，`interrupt()` 订阅线程（`:67-78`）。
- 手动重连命令：`ChatService.reconnectRedis()` 先 `close()` 置空，再按 `REDIS_ENABLED` 重建并 `start()`（`ChatService.java:488-498`）。

### 3.5 消息封装与动作（线格式）

编码为**单个 JSON 对象、唯一键 `data`、值为字符串数组**：`{"data":["a","b",…]}`（`RedisEnvelopeCodec.encode`，`:15-24`）。

| 动作 `data[0]` | 元素布局 | 发送点 | 接收点 |
|---|---|---|---|
| `BroadcastRaw` | `[1]`发送者 UUID（36 位带连字符，控制台为 NIL）、`[2]`组件 JSON、`[3]`监听权限、`[4]`doubleTransfer(`"true"`/`"false"`)、`[5]`端口列表以 `;` 连接、`[6]`fallback 纯文本、`[7]`发送者名、`[8]`被提及玩家名以 `,` 连接 | `ChatService.java:604-614`（9 元素）；控制台 `:126-135`（仅 7 元素，无 `[7][8]`） | `:996-1026` |
| `ForwardMessage`,`SendPrivateRaw`,… | `[0]="ForwardMessage"`、`[1]="SendPrivateRaw"`、`[2]`目标名、`[3]`发送者名、`[4]`接收方组件 JSON、`[5]`fallback、`[6]`消息组件 JSON | `TrChatProtocol.forwardPrivate`（`TrChatProtocol.java:44-60`），调用点 `ChatService.java:214-220` | `unwrap` 后 `:1028-1052` |
| `UpdateNames` | `[1]`serverId、`[2]`名字列表（`,`）、`[3]`显示名列表（`,`，空白写 `#`）、`[4]`UUID 列表（`,`） | `ChatService.java:1141-1147`；无人在线时发 `("UpdateNames", serverId, "", "#", NIL_UUID)`（`:1126-1128`） | `:1073-1099` |
| `GlobalMute` | `[1]` = `"on"` / `"off"` | `ChatService.java:332-336` | `:982-986` |
| `SendLang` | `[1]`目标玩家名、`[2]`语言键、`[3..]`参数（`,` 连接） | `ChatService.java:225-231` | `:1101-1115` |

- 接收分发 `handleRedisMessage`（`ChatService.java:972-994`）：先 `unwrap`，空则 return；未知动作仅 DEBUG `"Ignoring unsupported Bukkit Redis action '{}'"`；单个动作抛 `RuntimeException` 只 WARN 不中断。
- `UpdateNames` 接收：`serverId` 等于本服 `chat.serverId`（默认 25565）时忽略（`:1077-1080`）；`displayNames[i] == "#"` 时回落到 `names[i]`；UUID 解析失败跳过该条（`:1089-1096`）；快照带 `System.nanoTime()` 时间戳存入 `remotePlayers`（`:1098`）。
- 远端玩家 TTL：`REMOTE_PLAYER_TTL = Duration.ofSeconds(35)`（`ChatService.java:43`），`expireRemotePlayers()` 清理（`:1177-1180`）。
- 心跳：`tick()` 每 200 ticks（10 秒）`publishPlayerNames()` + `expireRemotePlayers()`（`:508-517`）；玩家进出时经 `playerListChanged()` → `server.execute(this::publishPlayerNames)` 立即广播（`:541-543`）。
- `receiveBroadcast`：`size<3` 直接丢弃；`size>5 && data[5]` 非空白时，用 `;` 切分后必须包含本服 `chat.serverId` 才继续（`:1000-1006`）；向有 `data[3]` 权限且未忽略发送者的在线玩家发送，并触发提及通知（`:1015-1025`）。
- `receivePrivate`：`size<4` 丢弃；按 `data[1]` 查本地目标，`data[2]` 为发送者名；目标被忽略时不投递但仍通知窥屏；`data[5]` 存在且非空白则作为窥屏渲染文本，否则回落到 `data[3]`（`:1028-1052`）。
- 私聊仅当本地目标不存在时才走 Redis（`ChatService.java:187-190`, `:214-223`），且要求 `processed.crossServerSafe()`，否则提示 `Redis-Unsafe-Item`。

### 3.6 防回环 / Double-Transfer

- **前缀剥离 `unwrap`**（`ChatService.java:1191-1197`）：只要 `size>1` 且首元素为 `"ForwardMessage"` 就反复剥离，使嵌套转发信封最终落到内层真实动作。
- **端口闸门**：`BroadcastRaw` 的 `data[5]` 承载 `channel.options().ports()`（`ChannelDefinition.java:35`，以 `;` 连接，`ChatService.java:610`）；接收端仅当自身 `chat.serverId` 在该列表中才处理（`:1000-1006`），从而避免多服互转风暴。
- `data[4]` 的 `doubleTransfer`（配置 `Options.Double-Transfer`，默认 `false`；`Global.yml` 为 `true`）**本实现只写不读**，接收端不以它做判断。
- 收到消息后**不再 `publish`**，故不存在本地回声；`publish` 返回值要求订阅者数 > 0，广播成功即提前 return（`:615-617`），不再走本地广播。

### 3.7 与其他部分的交互点

| 交互 | 位置 |
|---|---|
| 频道开关：`Options.Proxy` → `channel.options().redis()`；`Options.Force-Proxy` → `forceRedis` | `ChannelDefinition.java:32-33`；`ChatService.java:603,615-622` |
| Redis 不可用时：强制代理 → `Redis-Force-Unavailable` 且不发送；否则提示 `Redis-Fallback` 后本地广播 | `ChatService.java:618-623` |
| 私聊不可用 → `Redis-Private-Unavailable` | `ChatService.java:221` |
| 被忽略玩家过滤（本地与远端广播） | `ChatService.java:1017`（广播）、`:200`（本地私聊）、`:1038-1039`（远端私聊） |
| 全局禁言跨服同步 `GlobalMute` | `ChatService.java:332-336` / `:982-986` |
| 状态命令展示连接态（`Status-State-Connected` / `Status-State-Reconnecting`） | `TrChatCommands.java:281-285` |
| 手动重连 `/trchat redis reconnect`（`reconnect` 分支） | `TrChatCommands.java:460-469` |

---

## 4. 协议兼容（`protocol/`）

### 4.1 `TrChatMessage`（`protocol/TrChatMessage.java`）

- `record TrChatMessage(List<String> data)`；构造时 `List.copyOf`；`data` 为空抛 `IllegalArgumentException("TrChat message data must not be empty")`（`:7-12`）。
- `of(String...)`（`:14-16`）；`type()` = `data[0]`（`:18-24`）。

### 4.2 `TrChatProtocol`（`protocol/TrChatProtocol.java`）

| 成员 | 事实 | 行 |
|---|---|---|
| `NIL_UUID` | `new UUID(0L,0L)` → `"00000000-0000-0000-0000-000000000000"` | `:10` |
| `formatUuid(UUID)` | `uuid.toString()`，即 36 位带连字符规范式（Bukkit 侧 FastUUID `parseUUID()` 严格要求此格式） | `:15-19` |
| `parseUuid(String)` | `null`→`null`；`trim()`；长度 32 时插入连字符；`UUID.fromString` 失败 → `null`（不抛） | `:25-42` |
| `isCrossServerSafeItemNamespace(String)` | 仅 `"minecraft"` 返回 true（跨服禁止 mod 物品组件） | `:21-23` |
| `forwardPrivate(target, sender, receiverComponent, fallback, messageComponent)` | 产出 7 元素 `ForwardMessage` + `SendPrivateRaw` 信封 | `:44-60` |
| `emptyPlayerNames(serverId)` | `("UpdateNames", serverId, "", "#", formatUuid(NIL_UUID))`，用于清空本服远端快照 | `:62-70` |

### 4.3 `RedisEnvelopeCodec` 线格式（`protocol/RedisEnvelopeCodec.java`）

- **编码**：手工拼 JSON，输出严格形如 `{"data":["…","…"]}`（`:15-24`）。
- **转义**：`"`→`\"`、`\`→`\\`、`\b`、`\f`、`\n`、`\r`、`\t`，其余 `< 0x20` → `\u%04x`（小写十六进制）；**非 ASCII 字符原样输出**（`:37-59`）。
- **无 base64、无 gzip**：`data` 元素是原始 UTF-8 JSON 字符串；组件 JSON 本身以字符串内嵌（二次转义）。
- **解码**：容忍额外键（`skipValue` 跳过，`:86`, `:161-186`）；`data` 允许为数组**或单个标量字符串**（Bukkit `ArrayConverter` 可能产出标量；`:84`）；要求解析结束后 `cursor == length` 且 `data` 非空，否则 `IllegalArgumentException("Missing or empty data field at character N")`（`:95-97`）；支持 `\uXXXX` 转义（`:144-154`）；非法转义/未闭合字符串抛 `IllegalArgumentException`（`:155`, `:158`）。
- `quote(String)` 为公开辅助方法（`:31-35`）。

### 4.4 Bukkit / Bungee / Velocity 兼容要求

- 注释明确：编码的是「TrChat Bukkit 2.4.9 中 TabooLib Alkaid Redis 使用的完全相同的 `{"data":[…]}` 信封」（`:6-9`）。
- 模组描述同样声明与 TrChat Bukkit 2.4.9 线格式兼容（`src/main/templates/META-INF/neoforge.mods.toml` description、`fabric.mod.json` description）。
- `channel` 默认值 `"trchat-message"`，注释要求「与 Bukkit 版本互通时不要修改」（`TrChatConfig.java:158-160`）。
- `chat.serverId` 必须使用服务器端口数值以匹配 Bukkit 协议（`TrChatConfig.java:56-58`；默认 `25565`）。
- 跨服物品安全：只有 `minecraft:` 命名空间的物品组件允许跨服（`TrChatProtocol.java:21-23`；`ChatFunctionService.java:459`）。
- 本实现**不**实现 Bungee/Velocity 插件通道，仅 Redis 为唯一跨服传输（`TrChatConfig.java:143`；`TrChatServerEvents.java:47`）。

---

## 5. 更新检查（`update/`）

### 5.1 请求与频率（`update/UpdateChecker.java`）

| 项目 | 值 | 行 |
|---|---|---|
| API URL | `https://api.github.com/repos/Aruvelut-123/TrChat-Mod/releases/latest` | `:31-33` |
| 兜底发布页 | `https://github.com/Aruvelut-123/TrChat-Mod/releases` | `:34-35` |
| HTTP 客户端 | `connectTimeout(30s)`，`followRedirects(NORMAL)` | `:41-44` |
| 请求头 | `Accept: application/vnd.github+json`、`User-Agent: TrChat-Mod/<当前版本>` | `:99-100` |
| 请求超时 | 30 秒 | `:98` |
| 调度 | `scheduleWithFixedDelay(check, 1, UPDATE_CHECK_INTERVAL_MINUTES, MINUTES)`，**初始延迟 1 分钟**，固定延迟（非固定频率） | `:63-70` |
| 间隔默认/范围 | 15 分钟 / 1..1440 | `TrChatConfig.java:137-140` |
| 开关 | `updates.enabled` 默认 `true`；为 false 时**不创建** `UpdateChecker` | `TrChatConfig.java:133-136`；`TrChatServerEvents.java:43-46` |
| 线程 | 单线程守护调度器，线程名 `"TrChat Update Checker"` | `:45-49` |
| 并发保护 | `AtomicBoolean checking` CAS，重入直接返回 | `:50`, `:93-95` |
| 只提醒 | 不下载任何文件（配置注释明确） | `TrChatConfig.java:129-132` |

- `check()`（`:92-145`）：非 2xx → `IllegalStateException("GitHub API returned HTTP N")`；解析 `tag_name`、`html_url`（缺失回落 `RELEASES_URL`）、`body`（`isJsonNull` 视为空串）；`latest = SemanticVersion.parse(tag)`；`latest > current` 时构造 `ReleaseInfo(tag 去掉开头 v/V, page, ReleaseNotes.normalize(body))`，仅当与既有 `available` 不等（record `equals`）才 `notified.clear()` 并 `server.execute(() -> notifyAvailable(release))`；否则 `available = null`，并在 `!reportedCurrent` 时打印一次 `"… is newer than the latest GitHub release {}"` 或 `"… is up to date."`。异常 WARN `"Unable to check TrChat Mod updates: {}"`；`InterruptedException` 恢复中断标志。
- 通知对象：仅在线且通过 `TrChatPermissions.check(player, "trchat.admin")` 的玩家；每个 UUID 只通知一次（`notified` 集合，`:51`, `:72-90`）；新版本出现时清空 `notified` 以便重新通知。
- `notifyAvailable` 先 WARN 日志（含版本、URL、note 逐行拼接），再遍历 `server.getPlayerList().getPlayers()` 调 `notifyPlayer`（`:147-160`）。
- 玩家加入时调用：`TrChatServerEvents.onPlayerLogin` → `updateChecker.notifyPlayer(player)`（`TrChatServerEvents.java:83-85`）。
- 通知内容渲染顺序（`:79-88`）：`Updater-Available` 头部 → `Updater-Changelog` → 若 notes 为空则 `Updater-Changelog-Empty`，否则逐行 `ReleaseNoteRenderer.render(line)` → `Status-Footer`。头部附 `Updater-Link-Prefix` + `Updater-Link`（点击打开 `release.url()`，悬停 `Updater-Link-Hover`）（`:162-185`）。
- `close()`：`executor.shutdownNow()` + `notified.clear()`（`:187-191`）。`ReleaseInfo` 为 `record`（`:193-194`）。

### 5.2 `SemanticVersion` 完整比较规则（`update/SemanticVersion.java`）

解析（`:17-39`）：
1. `null` → 空串；`trim()`；**整体转小写**（`Locale.ROOT`）。
2. 去掉开头单个 `v`。
3. 在第一个 `+` 处截断：**构建号（build metadata）被完全丢弃**，不参与比较。
4. 在第一个 `-` 处切成「数字段」与「预发布段」（`split("-", 2)`）。
5. 数字段按 `.` 切分，每段用正则 `^(\d+).*$` 取开头连续数字后 `parseInt`；解析失败记 `0`（因此 `1.2.x` → `[1,2,0]`）；若结果为空补一个 `0`。
6. 预发布段：不存在或全空白 → 空列表；否则按 `.` 切分为字符串列表（不补零、不做数字归一）。

比较（`:41-79`）：
1. **数字段**：按较长者长度补 `0` 后逐位 `Integer.compare`；首个非零差值即结果。
2. **预发布**：两者皆空 → 相等；仅一方为空 → **空预发布者更大**（正式版 > 预发布版）。
3. 逐元素比较预发布：若 `left` 先耗尽 → `-1`；若 `right` 先耗尽 → `1`（**短的预发布更小**，如 `1.0-a` < `1.0-a.1`）。
4. 元素比较：双方均为纯数字 → `Long.compare`；一方数字一方非数字 → **数字方更小**（`-1`）；其余 → `String.compareTo`（已在解析阶段小写化）。
5. 全部相等 → `0`。

`parse` 永不抛异常；`compareTo` 无 `null` 检查。类与构造器为包私有（`final class`，`:7-15`）。

### 5.3 `ReleaseNotes`（`update/ReleaseNotes.java`）

- `normalize(body)`（`:18-36`）：`null`/全空白 → 空列表；`\r\n`、`\r` 统一为 `\n`；`split("\n", -1)`；去掉首尾空白行；其余每行仅 `stripTrailing()`（**保留行首空白**）；返回不可变列表。
- `parseLine(line)`（`:38-55`）判定顺序：**三级标题先于二级标题**；正则 `LEVEL_THREE_HEADING = ^\s*###(?!#)\s+(.+?)\s*$`、`LEVEL_TWO_HEADING = ^\s*##(?!#)\s+(.+?)\s*$`、`LIST_ITEM = ^\s*-\s+(.+?)\s*$`；空白 → `BLANK`；其余 → `TEXT`。
- `headingText()` 去掉尾部 `\s+#+\s*$` 再 `strip()`（`:57-59`）。
- `LineType`：`LEVEL_TWO_HEADING`、`LEVEL_THREE_HEADING`、`LIST_ITEM`、`TEXT`、`BLANK`（`:61-67`）。

### 5.4 `ReleaseNoteRenderer`（`update/ReleaseNoteRenderer.java:11-24`）

| LineType | 渲染 |
|---|---|
| `LEVEL_TWO_HEADING` | 字面量 + `AQUA` + `BOLD` |
| `LEVEL_THREE_HEADING` | 字面量 + `YELLOW` + `BOLD` |
| `LIST_ITEM` | `"  • "`（`DARK_GRAY`）+ 文本（`GRAY`） |
| `TEXT` | 文本（`GRAY`） |
| `BLANK` | `Component.empty()` |

### 5.5 Pumpkin 移植偏差（`pumpkin/src/updater.rs`）

* **线程模型**：Mod 用独立后台线程 `TrChat Update Checker`（`UpdateChecker.java:45-49`）跑 GET；WASI guest 是单线程，故改为宿主的重复任务（`schedule_repeating_task`，首次延迟 1 分钟 = 1200 tick，周期 = `updates.intervalMinutes` 经 `clamp(1, 1440)`），请求在 `wasi:http` 上阻塞该任务至多 30 秒；`AtomicBool checking` 的 CAS（对应 `:50, :93-95`）保证同刻只有一个请求在飞。
* **去重键**：Mod 的 `notified` 集合按玩家 **UUID**（`:86-89`）；WIT 的 `uuid` 类型没有字符串形式，故按**小写玩家名**去重（与 `chat.rs` 的 `PLAYER_STATES` 同一替代）。
* **通知呈现**：WIT 反馈是「一行一个组件」且没有 click / hover 动作，所以 `Updater-Link-Prefix` + `Updater-Link` 以纯文本拼接后成一行，`Updater-Link-Hover` 被解析但**不参与渲染**；`Updater-Available` 本身是两行文本、Mod 又在其后追加换行与链接块，因此 Rust 侧把整个通知块拆成多个组件顺序发送。
* **状态与日志**：`available` / `checking` / `reportedCurrent` 三个状态与 Mod 一一对应；「已是最新」与「比最新发布更新」两条 INFO 各只打印一次，文案前缀由 Mod 的 `TrChat Mod ...` 改为移植版统一的 `[TrChat] ...`。
* **权限门**：`notifyPlayer` 的判定等价于 Mod 的 `TrChatPermissions.check(player, "trchat.admin")`——查 `trchat.admin`（经 `perms::node` 补齐为 `trchat:trchat.admin`，注册默认值 `Op(Two)`，故「OP2 或被显式授权」都通过）。
* **WASI 版本与权限**：宿主以 `wasmtime_wasi_http::p2::add_only_http_to_linker_async` 提供 `wasi:http@0.2.x`，插件必须声明 `permissions::HTTP_OUTBOUND`，否则宿主返回 `HttpRequestDenied`。guest 侧依赖 `wasip2` 被**精确锁定 `=1.0.2`（wasi 0.2.9）**：更新版本会把整份组件的 WASI 导入（含 std 自带的 `wasi:filesystem` 等）抬到 0.2.12，凭空抬高宿主门槛；锁到 0.2.9 后组件的导入版本与改动前完全一致。
* **依赖已装载的全局配置**：`start()` 读的是 `config::global_config()` 的 `updates.enabled` / `intervalMinutes`。而 `global_config()` 首次访问会用**默认快照**初始化 `OnceLock`，`init_global` 之后再也装不上——真机上表现为日志打印 `Update checker disabled (updates.enabled: false)`，尽管 `settings.yml` 写的是 `true`。`on_load` 因此固定为「先 `ChatManager::init`（内部 `init_global`）→ 再 `register_commands` → 最后 `start`」，`init_global` 失败时写 WARN。
* **可测性边界**：非 `wasm32` 构建下 `fetch_latest` 直接返回错误（打印与 Mod 失败路径相同的 WARN），使 `cargo test` 完全离线；语义版本比较、更新日志解析/渲染、通知块顺序、GitHub 载荷解析都有单元测试。
* **真机验证（已通过）**：Pumpkin `0.2.0+26.3-26.51` / Windows x64 上，插件加载后日志出现 `[plugin] [TrChat] Update checker started (every 15 minute(s)).`，首次检查经 `wasi:http` 成功拉取 `releases/latest` 并完成语义版本比较，打印 `[plugin] [TrChat] TrChat 2.5.4.1 is up to date.`——WASI 0.2.9 导入、`http.outbound` 权限与宿主链接器匹配全部确认。

---

## 6. 启动、重载与关闭流程

### 6.1 入口（`TrChatMod.java`）

| 加载器 | 入口 | 行为 | 行 |
|---|---|---|---|
| NeoForge | `@Mod("trchat")` 构造器 | `ConfigMigration.migrateIfNeeded()` → `registerConfig(COMMON, TrChatConfig.SPEC, "trchat/settings.toml")` → `NeoForge.EVENT_BUS.register(new TrChatServerEvents())` | `:7-27` |
| Forge | `@Mod("trchat")` 无参构造器 | 同上，注册 `TrChatServerEventsForge` | `:28-47` |
| Fabric | `DedicatedServerModInitializer.onInitializeServer` | `migrateIfNeeded()` → `new TrChatServerEventsFabric()` | `:48-62` |

- 常量：`MOD_ID = "trchat"`、`MOD_NAME = "TrChat Mod"`、`LOGGER = LoggerFactory.getLogger("TrChat")`（`:18-20`）。
- 版本：`gradle.properties` 中 `mod_version=2.5.4.1`、`minecraft_version=1.21.1`；`Platform.modVersion()` 由 `NeoForgePlatform` 从 `ModList` 取，缺失回落 `"development"`（`NeoForgePlatform.java:26-31`）。

### 6.2 事件监听器注册（`TrChatServerEvents.java`，NeoForge 分支）

| 事件 | 优先级 | 行为 | 行 |
|---|---|---|---|
| `PermissionGatherEvent.Nodes` | — | `TrChatPermissions.register(event)` | `:35-38` |
| `ServerStartedEvent` | — | 建 `ChatService(server, channels)`；`UPDATE_CHECK_ENABLED` 时建并 `start()` `UpdateChecker`；INFO 启动日志 | `:40-49` |
| `ServerStoppingEvent` | — | `updateChecker.close()` 置空 → `service.close()` 置空 | `:51-61` |
| `ServerChatEvent` | `HIGHEST` | 世界禁用则放行；否则 `setCanceled(true)` + `service.handleChat` | `:63-77` |
| `PlayerEvent.PlayerLoggedInEvent` | — | `service.playerJoined` + `updateChecker.notifyPlayer` | `:79-87` |
| `PlayerEvent.PlayerLoggedOutEvent` | — | `service.playerLeft` | `:89-94` |
| `LivingDamageEvent.Post` | — | `service.recordDamage` | `:96-105` |
| `CommandEvent` | `LOWEST` | `service.checkCommand` 失败则取消；`routePrivateAlias` 命中则取消 | `:107-121` |
| `AnvilUpdateEvent` | — | `service.checkAnvil` 失败则取消 | `:123-130` |
| `ChunkEvent.Load` / `Unload` | — | `service.chunkLoaded` / `chunkUnloaded` | `:132-144` |
| `ServerTickEvent.Post` | — | `service.tick()` | `:146-151` |
| `RegisterCommandsEvent` | — | `channels.reload()` + `registerCommands(dispatcher)` | `:153-158` |

### 6.3 `ChatService` 构造时加载的服务（`ChatService.java:66-82`，顺序即事实）

1. `channels.reload()`（`ChannelManager.reload()`，`ChannelManager.java:41`）
2. `SpecialChars.reload()`
3. `new ModerationService()` → 内部 `PlayerDataStore.initialize()` + `LanguageService.reload()`（`ModerationService.java:32-35`）
4. `new PlaceholderResolver(server, metrics, playerStats, moderation.languages())`
5. `new ChannelRenderer(placeholders)`
6. `new ChatFunctionService(server, languages)` + `functions.reload()`
7. `new FilterService(server, languages)` + `filters.reload()`
8. 对当前在线玩家逐个 `playerJoined(player)`（恢复频道成员关系）
9. `reconnectRedis()`

- 字段级即时创建：`metrics`、`playerStats`、`chatLogs = new ChatLogService()`（构造即执行一次 `deleteExpired()`）（`:47-54`；`ChatLogService.java:36`）。
- `reconnectRedis()`（`:488-498`）：先关旧桥置空；`REDIS_ENABLED` 为真才新建 `RedisBridge(RedisSettings.fromConfig(), msg -> server.execute(() -> handleRedisMessage(msg)))` 并 `start()`。

### 6.4 reload 重建的对象（`ChatService.reloadConfiguration`，`:348-364`）

| 步骤 | 对象 | 失败标记 |
|---|---|---|
| 1 | `channels.reload()`；返回 `<0` 立即 `ReloadResult(false, -1, ["channels"])` | `channels` |
| 2 | 对每个在线玩家 `restoreChannelMembership(player)`（重新校验加入的频道与当前频道） | — |
| 3 | `functions.reload()` | `function.yml` |
| 4 | `filters.reload()` | `filter.yml` |
| 5 | `moderation.reloadLanguages()` | `lang` |
| 6 | `SpecialChars.reload()` | — |
| 7 | `reconnectRedis()`（**关闭并重建 Redis 桥**） | — |

- 返回 `ReloadResult(boolean success, int channelCount, List<String> failedSections)`（`:1226-1227`）。
- 命令侧（`TrChatCommands.reload`，`:430-458`）：`channelCount < 0` → `Reload-Failed`；否则 `registerDynamicCommands(commandDispatcher)` 并 `sendCommands(player)` 给全体；`!success` → `Reload-Partial`，成功 → `Reload-Success`。
- **reload 不重建**：`PlayerDataStore`、`ChatLogService`、`ModerationService` 实例与 `states` 缓存、`UpdateChecker`（间隔变更需重启生效）。

### 6.5 关闭时保存的内容（`ChatService.close`，`:545-562`，顺序即事实）

1. `publishPlayerNames()`（Redis 尚在，广播一次最终在线名单）
2. `redis.close()` 并置 `null`
3. 对每个在线玩家 `persistChannelMembership(player)` → `moderation.setChannels(...)` → `store.saveAsync`
4. 清空 `chatStates`、`activeChannels`、`joinedChannels`、`remotePlayers`、`lastPrivateSender`
5. `chatLogs.close()` → `flush()` 落盘（`ChatLogService.java:117-120`）
6. `moderation.close()` → `store.close()`（等待队列最多 10 秒）→ 逐个同步 `store.save` → `states.clear()`（`ModerationService.java:207-212`）

- `UpdateChecker.close()` 在事件层先于 `service.close()` 调用（`TrChatServerEvents.java:52-61`）。

---

## 7. 关键结论与实现时必须注意的坑

1. **Redis 是唯一跨服传输**，线格式固定为 `{"data":[…字符串…]}`；`data` 元素是**原始 UTF-8 字符串**，无 base64、无 gzip，仅 JSON 转义。Rust 侧必须实现等价转义表与「单标量 `data` 也接受」的宽容解码。
2. **频道名默认 `trchat-message`，`chat.serverId` 默认 25565（应等于服务端口）**：这两个值是 Bukkit 互通的硬约束，不能改默认。
3. **`BroadcastRaw` 元素位置是硬契约**：`[5]`=端口列表（`;` 分隔）才是防回环闸门；`[4]`=doubleTransfer 本实现只写不读。9 元素（玩家）与 7 元素（控制台）两种长度都要兼容。
4. **`unwrap` 必须先剥离前导 `ForwardMessage`**（可能多层），否则私聊信封会被判为未知动作丢弃。
5. **`UpdateNames` 收到自己的 serverId 要忽略**；`displayNames[i]=="#"` 回落名字；远端玩家缓存 TTL 35 秒、心跳 200 ticks（10 秒）；无人在线时发 `("UpdateNames", serverId, "", "#", NIL_UUID)` 清空快照，不可发空数组。
6. **玩家数据在首次 `load` 未命中时会立刻写库**（`empty` + `save`），因此「查无此人」也会产生一行 `trchat_player_state`。
7. **`joinedChannels` 与 `ignoredPlayers` 采用「先 DELETE 再批量 INSERT」全量覆盖**，不是增量更新；Rust 侧需在同一事务内完成。
8. **`chatColor` 规范化只允许 `[0-9a-f]`**，其余（含 `&a`、`red`）一律存 `""`；`activeChannel`/`channel_id` 一律小写。
9. **`PlayerDataStore` 的 `Type: JDBC` 分支是缺陷**：`:84-93` 只赋值 `table`/`channelTable`，`ignoredTable`/`preferenceTable` 保持 `null`，随后 `loadIgnoredPlayers`/`savePreferences` 会 NPE。移植时要么补全表名，要么明确不支持自定义 JDBC。
10. **`save()` 无连接池、每次新建 JDBC 连接**，且写失败只记日志不抛错——Rust 侧不要假设写一定成功，也不要照搬「静默吞异常」。
11. **`ModerationService.close()` 先 `store.close()`（关线程池）再逐个同步 `save`**：顺序不能反，否则异步队列会被丢弃。
12. **聊天日志是纯文本按天文件**（`logs/yyyy-MM-dd.txt`，行首 `HH:mm:ss`），5 分钟 flush、1 小时清理、`retentionDays=0` 表示**永不删除**；无查询 API，无 gzip 轮转。
13. **`SemanticVersion` 丢弃 `+build` 元数据、整体小写、预发布按段比较且短者更小、数字段不足补 0**；`1.2.x` 会被解析成 `1.2.0`。
14. **更新检查初始延迟 1 分钟、固定延迟 15 分钟、只通知 `trchat.admin` 且每人一次**；`notified` 只在新版本变化时清空；非 2xx 视为错误。
15. **reload 不重建 `PlayerDataStore` / `ChatLogService` / `ModerationService` / `UpdateChecker`**，但**会重建 Redis 桥**；`PlayerDataStore.initialize()` 只在启动时调用一次，表结构无迁移（仅 `CREATE TABLE IF NOT EXISTS`）。
