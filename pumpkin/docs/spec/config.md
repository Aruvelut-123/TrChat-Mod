# TrChat 配置层规格（供 Pumpkin WASM 重实现）

> 只描述事实。键名逐字准确；`来源` 列为仓库相对路径 + 行号。
> **重要前提**：任务点名的 `src/main/resources/defaults/*` 与 `src/main/java/.../config/*` 是
> **本仓库 Mod 侧（NeoForge/Fabric）自己的重实现**，并非 Bukkit v2 上游源码。
> 真正的 Bukkit v2 上游位于 `project/runtime-bukkit/`（Kotlin），两者 schema **不一致**。
> 下文以「M」标记 Mod 侧实现、「U」标记 Bukkit v2 上游（`git show upstream/v2:...`）。

---

## 1. 配置文件清单

| 文件 | 用途 | 来源 |
|---|---|---|
| `defaults/settings.yml` | 聊天/日志/更新/Redis 全局设置 | `src/main/resources/defaults/settings.yml:1-69` |
| `defaults/datasource.yml` | 数据源（仅 M 侧存在） | `src/main/resources/defaults/datasource.yml:1-59` |
| `defaults/special-chars.yml` | 特殊字符（彩色 emoji）白名单 | `src/main/resources/defaults/special-chars.yml:1-7` |
| `defaults/channels/Schema.yml` | 频道键全集 schema（不注册为频道） | `.../channels/Schema.yml:1-20` |
| `defaults/channels/{Normal,Global,Staff,Private}.yml` | 内置频道 | `.../channels/*.yml` |
| `defaults/channels/Example.yml` | 参考样例，**永不注册** | `.../channels/Example.yml:1-129` |
| `defaults/lang/{zh_CN,en_US,es_ES}.yml` | 语言文件 | `.../defaults/lang/` |
| `defaults/{filter,function}.yml` | 过滤/功能（不在本规格范围） | `.../defaults/` |
| `project/runtime-bukkit/.../settings.yml` | **U**：DB/Redis/拦截均在此文件 | `upstream/v2:.../resources/settings.yml` |
| `project/runtime-bukkit/.../channels/*.yml` | **U**：仅 4 个频道，无 Schema/Example | `upstream/v2:.../resources/channels/` |
| `project/runtime-bukkit/.../special-chars.yml` | **U**：同键名 `SpecialChars` | `upstream/v2:.../resources/special-chars.yml` |

**U 侧不存在 `datasource.yml`**；数据库配置在 `settings.yml` 的 `Database`/`Redis` 段。

---

## 2. `settings.yml` 完整键清单（M 侧）

类型/范围来自 `TrChatConfig.java` 的 `defineInRange`（NeoForge 分支首个/末个 `define` 在行 58/160，Forge 分支在行 219/318）。两分支由 Stonecutter 条件编译切换，标记为 `//? if neoforge`（行 8）、`//? } else if forge`（行 169）、`//? } else`（行 327）、`//? }`（行 462）；Fabric 分支（行 333-461）无 spec，仅惰性读取 YAML。

| 键路径 | 类型 | 默认值 | 单位 | 范围 |
|---|---|---|---|---|
| `chat.serverId` | Int | `25565` | — | 1–65535 |
| `chat.serverName` | String | `"A Minecraft Server"` | — | — |
| `chat.defaultLanguage` | String | `"zh_CN"` | — | — |
| `chat.globalPrefix` | String | `"!all"` | — | — |
| `chat.messageMaxLength` | Int | `256` | 字符 | 1–32767 |
| `chat.cooldownMillis` | Int | `2000` | ms | 0–600000 |
| `chat.antiRepeatSimilarity` | Double | `0.85` | 比率 | 0.0–1.0 |
| `chat.antiRepeatMaxPerPeriod` | Int | `0` | 条 | 0–60000 |
| `chat.antiRepeatPeriodMillis` | Int | `60000` | ms | 0–86400000 |
| `chat.antiRepeatCompareAll` | Bool | `false` | — | — |
| `chat.antiHighFrequencyMaxPerPeriod` | Int | `0` | 条 | 0–60000 |
| `chat.antiHighFrequencyPeriodMillis` | Int | `60000` | ms | 0–86400000 |
| `chat.antiDuplicatePhraseMaxRepeat` | Int | `0` | 次 | 0–100 |
| `chat.antiDuplicatePhraseWhitelist` | List\<String\> | `["哈","6","?","？","!","！"]` | — | — |
| `chat.blockedWords` | List\<String\> | `[]` | — | — |
| `chat.filterReplacement` | String | `"*"` | — | — |
| `chat.disabledWorlds` | List\<String\> | `[]` | — | 正则，忽略大小写 |
| `logging.normalMessageFormat` | String | `"[{0}] {1}: {2}"` | — | — |
| `logging.privateMessageFormat` | String | `"[{0}] {1} -> {2}: {3}"` | — | — |
| `logging.retentionDays` | Int | `0` | 天 | 0–36500（0=不删除） |
| `updates.enabled` | Bool | `true` | — | — |
| `updates.intervalMinutes` | Int | `15` | 分 | 1–1440 |
| `redis.enabled` | Bool | `false` | — | — |
| `redis.host` | String | `"127.0.0.1"` | — | — |
| `redis.port` | Int | `6379` | — | 1–65535 |
| `redis.username` | String | `""` | — | — |
| `redis.password` | String | `""` | — | — |
| `redis.database` | Int | `0` | — | 0–15 |
| `redis.connectTimeoutMillis` | Int | `3000` | ms | 100–60000 |
| `redis.socketTimeoutMillis` | Int | `0` | ms | 0–600000 |
| `redis.reconnectDelayMillis` | Int | `3000` | ms | 100–60000 |
| `redis.channel` | String | `"trchat-message"` | — | 与 Bukkit 互操作时不可改 |

- 语义（以 `settings.yml` 注释为准，行 14-26、45）：`antiRepeatSimilarity: 0` 关闭反重复；`antiRepeatMaxPerPeriod: 0` 表示**立即拦截相似消息**（每周期允许 0 条）；`antiHighFrequencyMaxPerPeriod: 0` 关闭高频限制；`antiDuplicatePhraseMaxRepeat: 0` 关闭重复短语限制；`retentionDays: 0` 不删除日志。**并非所有 `0` 都表示「关闭」**，`antiRepeatMaxPerPeriod` 是例外。
- **M 侧 Fabric 无 ModConfigSpec**（`TrChatConfig.java:335-461`）：按 `dotted.key` 惰性读取 YAML，**不校验范围**；类型转换异常时回退默认值（`Value.get()` 行 425-435）。
- 上游 `settings.yml` 键名完全不同（`Options.*`、`Channel.Default`、`Database.*`、`Redis.*`、`Chat.Interception.*`、`Color.*`、`MiniMessage.*`），且 `Cooldown`/`Period` 用 `2.0s`/`1m` 时长字符串，非毫秒整数。

---

## 3. `datasource.yml`（仅 M 侧）

| 键路径 | 类型 | 默认值 |
|---|---|---|
| `Type` | String | `SQLite` |
| `SQLite.File` | String | `data.db`（相对 `config/trchat`，可绝对路径） |
| `MySQL.Host` / `Port` / `Database` / `User` / `Password` | String | `127.0.0.1` / `3306` / `trchat` / `root` / `''` |
| `MySQL.Parameters` | String | `useUnicode=true&characterEncoding=utf8&useSSL=false&serverTimezone=UTC` |
| `MariaDB.Host` / `Port` / `Database` / `User` / `Password` | String | 同 MySQL |
| `MariaDB.Parameters` | String | `useUnicode=true&characterEncoding=utf8&useSsl=false` |
| `PostgreSQL.Host` / `Port` / `Database` / `User` / `Password` | String | `127.0.0.1` / `5432` / `trchat` / `postgres` / `''` |
| `PostgreSQL.Parameters` | String | `''` |
| `JDBC.Driver` / `Url` / `User` / `Password` | String | `''` |
| `JDBC.Table-Prefix` | String | `trchat_` |

- `Type` 匹配（`PlayerDataStore.java:64-93`）：`SQLite|Local` → SQLite；`MySQL`；`MariaDB`；`PostgreSQL|Postgres`；**其余一律走 `JDBC`**。
- 表名：SQLite 固定 `trchat_player_state` / `trchat_player_channels` / `trchat_player_ignored` / `trchat_player_preferences`（行 73-76）；网络型（MySQL/MariaDB/PostgreSQL）用 `{Table-Prefix}player_state` / `_channels` / `_ignored` / `_preferences`（行 152-156）。
- **`Type` 为 `JDBC` 时只设置 `table` 与 `channelTable`，`ignoredTable` / `preferenceTable` 保持默认字段值**（行 84-92 未赋值）。重实现时需与上游行为对齐或显式补全。
- JDBC URL 组装：`scheme://host:port/database` +（`Parameters` 非空则 `?` + 原文，行 148）；`Parameters` **不要带前导 `?`**。
- 表名经 `safeIdentifier(...)` 过滤；`jdbcUrl` 为空时抛 `IllegalArgumentException`（行 94）。

---

## 4. 频道（Channel）完整 schema

### 4.1 `Options`（记录声明 `ChannelDefinition.java:54-73`，装配于 `from()` 行 19-44；默认值取自 `Schema.yml`）

| 键 | 类型 | M 默认 | U 默认 | 语义 |
|---|---|---|---|---|
| `Join-Permission` | String | `''` | `''` | 加入/发言所需权限；空=所有人 |
| `Listen-Permission` | String | `''` | `=Join-Permission` | 接收所需权限；空→运行期沿用 Join-Permission（`ChatService.java:787-792`） |
| `Speak-Condition` | String | `''` | 空 Condition | 空→回退检查 `Join-Permission`（`ChatService.java:780-785`） |
| `Always-Listen` | Bool | `false` | `true` | true 时无需加入即接收 |
| `Auto-Join` | Bool | `false` | **无独立字段**；仅作 `Always-Listen` 的缺省回退（`Loader.kt:109`） | 仅当玩家无已存频道成员关系时作默认频道；全局最多 1 个 |
| `Private` | Bool | `false` | `false` | 私聊频道，需 target；不可 `Auto-Join` |
| `Target` | String | `ALL` | `ALL` | `ALL`/`SELF`/`SINGLE_WORLD`/`WORLD`/`DISTANCE;<blocks>` |
| `Proxy` | Bool | `false` | `false` | Bukkit 兼容的 Redis 转发（非代理端连接） |
| `Force-Proxy` | Bool | `false` | `false` | Redis 不可用时拒绝发送 |
| `Double-Transfer` | Bool | `false` | **`true`** | Bukkit Redis 协议兼容标记 |
| `Ports` | M: List\<String\> / U: String | `[]` | `''` | 目标服务器 ID；空=全部 |
| `Disabled-Functions` | List\<String\> | `[]` | `[]` | 此频道禁用的 function 名 |

- **`Target` 解析**：M 先 `toUpperCase(Locale.ROOT)`，空白/null → `ALL`（`ChannelDefinition.java:68-72`），**未知值不在解析期报错**，而是在 `switch` 落入 `default -> true`（即视为全部可见，`ChatService.java:821-835`）；U 为 `uppercase().split(";")`，第 1 段做 `Type.valueOf`，第 2 段 `toInt`（`Loader.kt:111-114`），**未知值抛异常导致频道加载失败**。`distance` 缺省 `-1`；`DISTANCE` 用平方距离比较（`ChatService.java:825-827`）。
- **`Ports` 类型分歧**：M 是 YAML 列表；U 是**单个分号分隔字符串**再 `map{toInt}`（`Loader.kt:118`）。列表化会让上游配置解析成空。
- **U 额外键**（M 未实现）：`Filter-Before-Sending`(false)、`Send-To-Discord`(!isPrivate)、`Receive-From-Discord`(true)、`Discord-Channel`("")（`Loader.kt:120-123`）。
- `strings()` 过滤规则：丢弃空白、`null`、`~`（`ChannelDefinition.java:185-190`）；`string()` 把 `~` 归一为空串（行 192-194）。

### 4.2 `Bindings`

| 键 | 类型 | 默认 | 语义 |
|---|---|---|---|
| `Prefix` | List\<String\> | `[]` | 聊天前缀；**最长匹配优先**（`ChannelManager.java:125-137`）；U 侧 Private 频道强制为 null |
| `Command` | List\<String\> | `[]` | 触发该频道的命令别名；`/trchat reload` 后立即刷新 |

> **Pumpkin deviation（§1.1，WASM guest 约束）**：WIT 无 `unregister-command`，guest 侧无法在 reload 时先注销旧别名再重新注册。因此 Pumpkin 加载时（`on_load`）静态注册一次，此后 reload 只刷新配置，**不再刷新命令别名**（`trchat-pumpkin` issue #6 跟踪）。若上游将来补上 WIT `unregister-command`，本 dev 应撤回，Pumpkin 即可实现真正的热刷新。

### 4.3 `Formats` / `Sender` / `Receiver` / `Console`

- `Formats`：公共频道格式；`Sender`/`Receiver`：仅 Private 频道使用；`Console`：后台日志格式。
- **U 侧 `Console` 缺失会直接失败**（取 `consoleFormat.firstOrNull()`，空则 `NO_FORMAT`，`Channel.kt:112-118`；**无 `Formats` 回退**）；**M 侧 `Console` 为空时回退到 `Formats`**（`ChannelRenderer.java:89`）。
- 每个格式条目字段：

| 字段 | 类型 | 默认 | 语义 |
|---|---|---|---|
| `condition` | String | 空/`~`=真 | 条件表达式，见 §5 |
| `priority` | Int | **M: `0`** / **U: `100`** | 排序键，见下方警告 |
| `prefix` | Map\<String, 组件 \| List\<组件\>\> | `{}` | 任意命名部件，按 YAML 顺序输出 |
| `msg` | Map | — | 见 4.5 |
| `suffix` | Map | `{}` | 同 `prefix` |

> **实现必须注意**：M 侧格式按 priority **降序**排序后取首个匹配（`ChannelDefinition.java:121`，`right - left`）；
> U 侧按 priority **升序**排序后取首个匹配（`Loader.kt:153,178` + `Channel.kt:162`）。
> 即同一份配置，M 选高优先级、U 选低优先级——**语义相反**。组件变体同样降序（M，行 137）/ 升序（U，`Loader.kt:231`）。

### 4.4 组件部件（`ComponentPart`，`ChannelDefinition.java:143-157`）

| 字段 | 语义 |
|---|---|
| `condition` | 该变体条件；`List` 形态下按 priority 选首个满足者 |
| `priority` | 变体排序键（M 降序 / U 升序） |
| `text` | 文本，支持 `&` 传统颜色与占位符 |
| `hover` | 悬浮文本（支持多行 `|-`） |
| `suggest` | 点击→插入命令到输入框 |
| `command` | 点击→执行命令 |
| `url` | 点击→打开链接 |
| `copy` | 点击→复制到剪贴板 |
| `file` | 点击→打开本地文件 |
| `insertion` | Shift 点击插入文本 |
| `font` | 资源字体（`ResourceLocation`） |

- **点击动作优先级**（M，`ChannelRenderer.java:203-273`）：`suggest` > `command` > `url` > `copy` > `file`，取第一个非空；`url` 会 `trim()` 并截到首个空格，且需通过 `isValidUrl`（即 `new URI(url)` 成功，行 282-287）校验，否则返回 `null`（不产生点击事件，行 233-247）。
- `insertion`/`font` 与点击事件可并存（行 186-199）。
- **U 额外字段**：`head`（头像，`Loader.kt:257`）与 `shadow`（阴影色，`Loader.kt:247`），M 未实现。

### 4.5 `msg`

| 键 | 类型 | 默认 | 语义 |
|---|---|---|---|
| `msg.default-color` | String | M: `"f"`（空白时）/ U: 必填 | 消息主体默认色；可写 `7`、`&7`、`§7`，M 会剥掉首个 `&`/`§` 并取首字符；非法色 → `WHITE`（`ChannelRenderer.java:105-114`） |
| `msg.special-char.enabled` | Bool | `false` | 启用特殊字符着色；M 同时接受大写 `Enabled`（`ChannelDefinition.java:163`） |
| `msg.special-char.special-char-color` | String | `"&f"` | 特殊字符包裹色；空白 → `&f`（行 169） |
| `msg.hover` | String | `''` | 消息悬浮文本；非空则加 `HoverEvent.ShowText` |

- 消息正文流程（`ChannelRenderer.java:116-133`）：解析占位符 → 剥离传统色码 → 前置 `&<color>` → 若 `specialCharsEnabled` 且消息含特殊字符则用 `SpecialChars.wrapSpecialChars(body, specialCharsColor, "&"+color)` 包裹。
- `special-char-color` 与 `default-color` 需成对考虑：包裹前缀用 special 色、后缀恢复默认色。

---

## 5. 条件表达式（condition）

**M 侧实现**：`src/main/java/me/arasple/mc/trchat/channel/ConditionEvaluator.java:9-47`。

| 写法 | 结果 | 行号 |
|---|---|---|
| `null` / 空白 / `~` | `true` | 20-22 |
| `player op` 或 `player is op` | 玩家 OP（1.21.11+ 用 `PermissionLevel.ADMINS`，否则 `hasPermissions(2)`） | 24-34 |
| `perm "node"` / `permission node` | 检查权限节点；引号可选（正则 `(?:perm\|permission)\s+["']?([^"'\s]+)["']?`，忽略大小写） | 11-14, 35-42 |
| 节点以 `*` 开头 | 剥掉 `*` 后再检查 | 38-40 |
| 前置 `!` | 递归取反 | 43-45 |
| 其他任意文本 | **`false`**（未知判定词静默不匹配） | 46 |

**U 侧实现**：`Condition.kt` → 空脚本 `true`；否则 `JavaScriptAgent.serialize` 判断是否为 JS，是则 `JavaScriptAgent.eval`，否则 `KetherHandler.eval`。**判定词体系完全不同**：U 支持 Kether 脚本/JS 与 `perm` 等 Kether action，M 只硬编码 3 种。

---

## 6. 配置加载与迁移

### 6.1 目录迁移（`ConfigMigration.java:29-64`）
- 一次性把 `config/trchat-neoforge/` 整体搬到 `config/trchat/`（递归，保留相对结构）。
- 若 `trchat` 已存在 → **跳过**并 WARNING，不合并、不覆盖。
- 文件用 `ATOMIC_MOVE`，不支持时退化为 `REPLACE_EXISTING`；全部搬完后删除旧目录。
- 失败仅记录 ERROR，不抛出。

### 6.2 YAML 同步（`YamlConfigSynchronizer.java:34-204`）
1. 文件不存在 → 把 bundled 默认资源原样复制过去（行 58-65）。
2. 读取默认、schema、当前三方；`reconcileMap(defaults, schema, current, path, openMapPaths)`。
3. **以默认键顺序重建输出**（`LinkedHashMap`，行 108-121）：
   - 键在默认中存在 → 递归 `reconcile`；当前缺失则复制默认值。
   - 键只在当前中存在：若 **schema 声明过** → 保留并按 schema 递归（**不会**因为缺失而被补进文件）；若 `openMapPaths` 含**父路径** → 原样保留；否则 **删除**。
4. 叶子值规则（行 140-164）：Map→递归；List→当前是 List 则保留用户列表，否则用默认；标量→**无条件保留用户值**（即使默认是标量而用户写成 Map，也保留，见行 161-163）。
5. 仅在结果与当前不等时才回写（行 68-71），写临时文件后 `ATOMIC_MOVE`（行 184-204）。
6. 解析限制：禁止重复键、码点上限 4 MiB、根必须是 mapping（行 90-99）。输出：BLOCK 流、缩进 2、`indicatorIndent 2`、不折行。

### 6.3 频道加载（`ChannelManager.java:41-97`）
- 目录 `config/trchat/channels/`；先同步 `Normal/Global/Staff/Private` 与 `Example`，再遍历 `*.yml`。
- **`Example.yml` 与 `Server.yml` 永不注册**（行 54-55）；每个频道用「同名默认 + `Schema.yml` 作为 schema」同步（行 159-170）。
- 频道 id 取文件名去 `.yml`，注册时**转小写**；`Normal` 缺失 → `IOException`（行 63-65）。
- `Auto-Join` 为 true 的频道 **>1 个即报错**（行 71-76）；`Auto-Join` 频道若 `Private` 也报错（行 83-88）。
- 前缀匹配：遍历全部频道全部前缀，取 `message.startsWith` 中**最长**者（行 125-137）。

### 6.4 Pumpkin 移植（`pumpkin/src/sync.rs`、`config.rs::seed_defaults`）
- `sync::synchronize(file, default, schema)` 实现 §6.2 的 1/3/4/5 条：文件不存在 → 原样复制 bundled 资源；
  以默认键顺序重建输出（`serde_yaml::Mapping` 保留插入顺序）——键在默认中存在则递归、当前缺失则取默认值；
  只在当前中存在的键，schema 声明过则保留并按 schema 递归，否则**删除**；叶子值 Map→递归、
  List→当前是 List 则保留用户列表、标量→无条件保留用户值。
- 只有结果与当前不等才回写（行 68-71），写 `<file>.yml.tmp` 后 `rename`，失败回退原地写。
- 与 Java 的差异：**没有 `openMapPaths` 参数**（四个调用点都传 `Set.of()`，形同未使用）；输出由 `serde_yaml`
  序列化（缩进 2、序列不额外缩进、多行标量走引号转义）。因此随包发行的默认文件保持**逐字节不变**
  （测试 `the_bundled_files_are_already_reconciled` 断言 settings/datasource/filter 与六个频道文件回环无改写）。
- 接入点与 Mod 一致：`settings.yml`（`TrChatConfig.java:391`）、`datasource.yml`（`PlayerDataStore.java:61`）、
  `filter.yml`（`FilterService.java:62`）与 `channels/*.yml`（`ChannelManager.java:164`）。
  `function.yml`（键是用户自定义命令，schema 无从声明）、`special-chars.yml`（`SpecialChars.java:38-45` 只补缺失文件）
  与 `lang/*.yml`（`LanguageService` 不经过同步器）仍只做缺失播种。
- 对齐失败只 `diag::warn` 后跳过，由随后的段读取器按 `settings.yml` / `filter.yml` / `channels` 上报
  （`reload_from_folder` 的 `ReloadOutcome`），整个加载不会被中止；损坏文件原样保留给运维排查。
- 频道文件同样对齐：同名 bundled 默认存在则用它，否则用 `Schema.yml`（`ChannelManager.synchronizeChannel:159`），
  `Server.yml` 跳过。补全默认值会改变部分语义：`Normal.yml` 只写 `Id: Normal` 时会被补上 bundled 的
  `Options.Auto-Join: true`（与 Mod 相同），因此该不变量的测试用例显式写 `Auto-Join: false`。

---

## 7. `SpecialChars`

- 文件键：`SpecialChars: []`，值类型 `List<String>`（M: `special-chars.yml:7`；U: 同键）。
- 加载（`SpecialChars.java:35-66`）：文件不存在则复制 bundled 默认；只认根 map 的 `SpecialChars` 列表；元素 `String.valueOf` 后**丢弃空白项**，收集为不可变 `Set`；任何异常 → 空集 + ERROR 日志。
- `hasSpecialChars`（行 72-85）：按 **Unicode 码点**遍历，逐码点查集合。
- `wrapSpecialChars(text, prefix, suffix)`（行 91-149）：
  - 连续特殊字符段只在**段首**插 `prefix`、**段尾**插 `suffix`（不逐字包裹）。
  - 扩展码点不断段：ZWJ `U+200D`、肤色修饰 `U+1F3FB–U+1F3FF`、变体选择符 `U+FE0F`。
  - 遇到手动颜色码 `&x`：复制该码及其后 1 字符；`&r`/`&R` 视为「无手动色」；若该段已带手动色则**跳过包裹**，保留玩家自选色。
  - 特殊处理：若 `&` 位于索引 0 且文本以 `suffix` 开头，则视为默认前缀而非手动色。
  - `prefix` 为 null/空白或集合为空 → 原样返回。
- 触发条件（`ChannelRenderer.java:118-126`）：`specialCharsEnabled` && 未提供预构建 Component && `hasSpecialChars(cleanMessage)`。

---

## 8. 关键结论（实现时必须注意的坑）

1. **任务点名的文件不是 Bukkit 上游**：`src/main/**` 是 Mod 重实现；Bukkit v2 在 `project/runtime-bukkit/`（Kotlin），键名与默认值大量不同，重实现前必须明确以哪套为兼容目标。
2. **priority 方向相反**：M 降序取首个（高优先级胜），U 升序取首个（低优先级胜）。移植时这是最容易静默出错的一处。
3. **默认值冲突**：`Always-Listen`（M false / U true）、`Double-Transfer`（M false / U true）、`priority` 缺省（M 0 / U 100）。U 侧 `Auto-Join` 被当作 `Always-Listen` 的回退，M 侧是独立键。
4. **`Ports` 类型冲突**：M 为 YAML 列表，U 为分号分隔字符串；二者互不兼容。
5. **`Listen-Permission`/`Speak-Condition` 空值语义**：都回退到 `Join-Permission`，不是「放行所有人」。
6. **同步器会删未知键**：只有 schema 声明或 `openMapPaths` 父路径下的键才保留；schema 键「存在则留、缺失不补」。
7. **条件判定词极少且未知即 false**：M 仅 `~`、`player op`/`player is op`、`perm`/`permission`、`!`；U 是 Kether/JS。不支持 `!` 之外的布尔组合。
8. **M 侧 Fabric 不校验范围**：settings 的 min/max 仅 NeoForge/Forge 生效，Fabric 越界值会被原样使用。
9. **`Example.yml`/`Server.yml` 必须硬排除**；`Normal` 缺失与 `Auto-Join` 重复是**致命错误**（加载失败，不是警告）。
10. **点击动作择一**：`suggest`>`command`>`url`>`copy`>`file`，且 `url` 必须能通过 URI 校验才生效。
11. **`SpecialChars` 按码点匹配并跳过 ZWJ/肤色/VS16**，且玩家手动颜色码优先于自动包裹色。
12. **Pumpkin 已对齐缺失键语义（§6.4）**：`settings.yml` / `datasource.yml` / `filter.yml` / `channels/*.yml`
    在解析前先与 bundled 默认对齐并把缺失键写回文件（即 M 侧 `YamlConfigSynchronizer` 的行为），
    因此不再落到 Rust 类型默认值（如 `chat.serverName` 缺失会补 `"A Minecraft Server"`）。
    `function.yml`、`special-chars.yml`、`lang/*.yml` 仍只做缺失播种（Mod 也不对它们对齐）。
    `#[serde(default)]` 保留为解析层兜底（对齐被跳过或用户运行期改文件时）。
