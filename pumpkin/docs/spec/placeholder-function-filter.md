# TrChat 内置占位符 / 聊天功能（function.yml）/ 过滤器（filter.yml）精确规格

> 目的：作为在 Pumpkin WASM（Rust）插件中重新实现这三块的**唯一事实来源**。
> 所有结论均来自本仓库 `v2` 分支（`pumpkin-experimental` 基线 `origin/v2`）的实际源码，
> 逐条标注来源文件与行号范围。**不含任何推测**；实现差异会在"坑"中显式说明。
>
> **PlaceholderAPI（PAPI）扩展明确不实现**，见 §2.9 的跳过清单。

---

## 0. 来源文件索引

| 领域 | 文件 | 行数 |
| --- | --- | --- |
| 占位符解析 | `src/main/java/me/arasple/mc/trchat/placeholder/PlaceholderResolver.java` | 1–653 |
| 占位符目录 | `src/main/java/me/arasple/mc/trchat/placeholder/PlaceholderCatalog.java` | 1–98 |
| 伤害统计 | `src/main/java/me/arasple/mc/trchat/placeholder/PlayerStatsTracker.java` | 1–33 |
| TPS 统计 | `src/main/java/me/arasple/mc/trchat/placeholder/ServerMetrics.java` | 1–42 |
| 聊天功能 | `src/main/java/me/arasple/mc/trchat/function/ChatFunctionService.java` | 1–915 |
| 命令控制 | `src/main/java/me/arasple/mc/trchat/function/CommandController.java` | 1–100 |
| 只读容器 | `src/main/java/me/arasple/mc/trchat/function/ReadOnlyChestMenu.java` | 1–58 |
| 过滤器服务 | `src/main/java/me/arasple/mc/trchat/filter/FilterService.java` | 1–296 |
| 文本匹配 | `src/main/java/me/arasple/mc/trchat/filter/TextFilter.java` | 1–111 |
| 默认配置 | `src/main/resources/defaults/function.yml` | 1–133 |
| 默认配置 | `src/main/resources/defaults/filter.yml` | 1–19 |

辅助引用：`lang/LanguageService.java`（本地化）、`channel/ConditionEvaluator.java`（condition 求值）、
`chat/ChatService.java`（调用顺序）、`chat/LegacyText.java`（`&` 颜色解析）、
`permission/TrChatPermissions.java`（权限回落）。

---

## 1. 占位符系统

### 1.1 解析管线（`PlaceholderResolver.java`）

| 步骤 | 行为 | 行号 |
| --- | --- | --- |
| 1 | `input == null \|\| input.isEmpty()` → 返回 `""` | 92–94 |
| 2 | 正则 `%([^%]+)%` 全局匹配（`PLACEHOLDER`） | 57, 95 |
| 3 | `token = matcher.group(1).trim().toLowerCase(Locale.ROOT)` | 98 |
| 4 | 优先查 `local` 表（键为**已小写**的 token）；命中即用 | 99–100 |
| 5 | 未命中 → `resolveToken(token, subject)` | 101 |
| 6 | 再经 `languages.translatePlaceholder(token, value)` 做本地化 | 102 |
| 7 | `Matcher.quoteReplacement`，`null` 视为 `""` | 104 |

关键事实：

* **语法**：`%` 与 `%` 之间不能含 `%`；未闭合的 `%` 原样保留（正则不匹配）。
* **大小写不敏感**：token 统一小写，因此 `%PLAYER_NAME%` 与 `%player_name%` 等价。
* **首尾空格被裁掉**：`% player_name %` 等价。
* **未知占位符 → 空字符串（被删除）**，不是原样保留（`resolveToken` 末尾 `return ""`，行 117）。
* `viewer` 参数在 `PlaceholderResolver` 内**完全未被使用**：`resolveToken(token, subject)`（行 101）。
  所有 `player_*` 一律以 **消息主体（subject）** 为准，与查看者无关。Pumpkin 实现可省略该参数。
* 三个公开重载（行 78–84）只是 `subject == viewer` / 空 local 的包装。

### 1.2 token 路由（行 110–118）

| 前缀 | 处理 | 行号 |
| --- | --- | --- |
| `player_` | `player(token.substring(7), player)` | 111–113 |
| `server_` | `server(token.substring(7))` | 114–116 |
| 其他 | `""` | 117 |

### 1.3 `server_*` 静态键（行 120–150）

| 占位符 | 取值 | 行号 |
| --- | --- | --- |
| `%server_name%` | `TrChatConfig.SERVER_NAME`（settings.yml `chat.serverName`） | 123 |
| `%server_online%` | 当前在线玩家数 | 124 |
| `%server_version%` | `server.getServerVersion()` | 125 |
| `%server_max_players%` | 最大玩家数 | 126 |
| `%server_unique_joins%` | `<world>/playerdata` 下 `*.dat` 文件计数 | 127, 435–445 |
| `%server_uptime%` | JVM uptime 秒 → `duration()` | 128, 490–506 |
| `%server_ram_used%` | `(total-free)/1048576` MiB | 129 |
| `%server_ram_free%` | `free/1048576` MiB | 130 |
| `%server_ram_total%` | `total/1048576` MiB | 131 |
| `%server_ram_max%` | `max/1048576` MiB | 132 |
| `%server_tps%` | `min(20, 1e9/max(5e7, getAverageTickTimeNanos()))`（≥1.20.5）；旧版 `min(20,1000/max(50,getAverageTickTime()))` | 133–137 |
| `%server_tps_1%` | `ServerMetrics.tps(1)` | 138 |
| `%server_tps_5%` | `ServerMetrics.tps(5)` | 139 |
| `%server_tps_15%` | `ServerMetrics.tps(15)` | 140 |
| `%server_tps_1_colored%` | `coloredTps(tps(1))` | 141 |
| `%server_tps_5_colored%` | `coloredTps(tps(5))` | 142 |
| `%server_tps_15_colored%` | `coloredTps(tps(15))` | 143 |
| `%server_has_whitelist%` | `yes`/`no` | 144 |
| `%server_total_chunks%` | 所有维度已加载区块数之和 | 145, 447–453 |
| `%server_total_living_entities%` | 所有维度 `LivingEntity` 计数 | 146, 455–465 |
| `%server_total_entities%` | 所有维度实体计数 | 147, 455–465 |

### 1.4 `server_*` 动态键（`dynamicServer`，行 152–178）

| 模式 | 语义 | 行号 |
| --- | --- | --- |
| `%server_online_<dim>%` | 遍历全部维度，`id.toString()` 或 `id.getPath()` 与 `<dim>` **忽略大小写**相等即返回该维度玩家数；全部不匹配返回 `-1` | 153–166 |
| `%server_time_<pattern>%` | `ZonedDateTime.now().format(DateTimeFormatter.ofPattern(pattern))`；非法 pattern 返回 `""` | 167–173 |
| `%server_countdown_<pattern>_<target>%` | 见下 | 174–176, 467–488 |
| 其他 | `""` | 177 |

`countdown` 细节（行 467–488）：

* 在参数中找**第一个** `_` 切分为 `pattern` 与 `target`；`_` 在首/尾或不存在 → 返回字面量 `invalid format and time`。
* 先按 `LocalDateTime` 解析，失败再按 `LocalDate`（当天 00:00）；时区 `ZoneId.systemDefault()`。
* 差值秒数 `<= 0` → `"0"`；否则 `duration(seconds)`。
* pattern/target 非法 → 返回字面量 `invalid date`。

### 1.5 `player_*` 动态前缀（行 180–200）

| 模式 | 语义 | 行号 |
| --- | --- | --- |
| `%player_ping_<name>%` | 目标离线返回 `0`，否则其延迟（ms） | 181–188 |
| `%player_has_permission_<node>%` | `yes`/`no`，走 `TrChatPermissions.check` | 189–191 |
| `%player_has_potioneffect_<id>%` | `yes`/`no`；`<id>` 小写，无 `:` 则补 `minecraft:` | 192–194, 384–398, 420–423 |
| `%player_item_in_hand_level_<enchant>%` | 主手附魔等级，未知附魔 `0` | 195–197, 400–418 |
| `%player_item_in_offhand_level_<enchant>%` | 副手附魔等级 | 198–200, 400–418 |

### 1.6 `player_*` 静态键（`player(...)` 大 switch，行 222–381）

以下按来源行号分组，名称**逐字**照抄。

**盔甲（行 224–235）**：`player_armor_helmet_name`、`player_armor_helmet_data`、`player_armor_helmet_durability`、
`player_armor_chestplate_name`、`player_armor_chestplate_data`、`player_armor_chestplate_durability`、
`player_armor_leggings_name`、`player_armor_leggings_data`、`player_armor_leggings_durability`、
`player_armor_boots_name`、`player_armor_boots_data`、`player_armor_boots_durability`。

**床 / 指南针（行 236–243, 255–262）**：`player_bed_x`、`player_bed_y`、`player_bed_z`、`player_bed_world`、
`player_compass_world`、`player_compass_x`、`player_compass_y`、`player_compass_z`。
床坐标为空（无重生点）时返回 `""`。

**位置 / 世界（行 244–246, 269–270, 327–328, 351–378）**：`player_biome`、`player_biome_capitalized`、
`player_block_underneath`、`player_direction`、`player_direction_xz`、`player_level`、`player_light_level`、
`player_time`、`player_time_offset`（恒为 `"0"`）、`player_world`、`player_world_type`、
`player_world_time_12`、`player_world_time_24`、`player_x`、`player_y`、`player_z`、`player_yaw`、`player_pitch`。

**状态布尔（行 223, 247, 280–283, 289–306）**：`player_allow_flight`、`player_can_pickup_items`、
`player_has_empty_slot`、`player_has_played_before`、`player_has_health_boost`、`player_online`（恒 `"yes"`）、
`player_is_whitelisted`、`player_is_banned`、`player_is_flying`、`player_is_sneaking`、`player_is_sprinting`、
`player_is_sleeping`、`player_is_inside_vehicle`、`player_is_op`。

**生命 / 饥饿 / 经验（行 268, 273–274, 277–287, 327, 329–333, 339, 345–348, 350, 357, 379）**：
`player_current_exp`、`player_exp`、`player_exp_to_level`、`player_total_exp`、`player_food_level`、
`player_saturation`、`player_health`、`player_health_boost`、`player_health_rounded`、`player_health_scale`、
`player_max_health`、`player_max_health_rounded`、`player_remaining_air`、`player_max_air`、
`player_max_no_damage_ticks`（恒 `"20"`）、`player_no_damage_ticks`、`player_last_damage`、
`player_absorption`、`player_sleep_ticks`、`player_seconds_lived`、`player_minutes_lived`、`player_ticks_lived`、
`player_fly_speed`、`player_walk_speed`。

**身份 / 显示（行 264–267, 271–272, 279, 288, 335–338, 358）**：`player_custom_name`、`player_displayname`、
`player_list_name`、`player_gamemode`、`player_ip`、`player_name`、`player_uuid`。

**物品（行 307–314）**：`player_item_in_hand`、`player_item_in_hand_name`、`player_item_in_hand_data`、
`player_item_in_hand_durability`、`player_item_in_offhand`、`player_item_in_offhand_name`、
`player_item_in_offhand_data`、`player_item_in_offhand_durability`、`player_empty_slots`。

**语言环境（行 316–323）**：`player_locale`、`player_locale_display_name`、`player_locale_short`、
`player_locale_country`、`player_locale_display_country`。

**会话时间（行 275–276, 325–326）**：`player_first_played`、`player_first_join`（同义）、
`player_first_played_formatted`、`player_first_join_date`（同义）、`player_last_played`、`player_last_join`（同义）、
`player_last_played_formatted`、`player_last_join_date`（同义）。

**天气（行 349, 360）**：`player_thunder_duration`、`player_weather_duration`。

**延迟（行 248–254, 341–344）**：`player_colored_ping`、`player_ping`。

**未匹配**：`default -> ""`（行 380）。

**Pumpkin 支持情况**（WASM 沙盒可及性）：

* **已原生实现**：`player_level`（经验等级）、`player_is_sleeping`（实体 pose）、`player_can_pickup_items`
  （`!spectator` 游戏模式）、`player_health_boost`（`max(0, maxHealth-20)`）、`player_health_scale`（`maxHealth`）、
  `player_has_health_boost`（`HEALTH_BOOST` 效果）、`player_block_underneath`（脚下方块，`registryName` 大写）、
  `player_armor_helmet/chestplate/leggings/boots_{name,data,durability}`（按槽位访问器，`data`/`durability`
  同手持物品恒为 `"0"`）、`player_current_exp`（`totalExperienceAtCurrentLevel` 公式，含进度条）。
* **仍解析为空**（WIT 无对应访问器）：`player_has_played_before`、`player_first_played`/`player_last_played`
  系列（读 `playerdata` 目录时间戳）、`player_sleep_ticks`（`getSleepTimer`）、`player_no_damage_ticks`、
  `player_last_damage`、`player_thunder_duration`/`player_weather_duration`（世界天气计时器）。
* **仍解析为空**（guest 无文件系统计数）：`server_unique_joins`（需遍历 `<world>/playerdata/*.dat`，
  §1.1 未知→空；与 `playerdata` 系列同一根因，显式 arm + 注释记录于 `placeholder.rs`）。

### 1.7 值格式化规则（实现必须逐字对齐）

| 函数 | 规则 | 行号 |
| --- | --- | --- |
| `bool` | `true`→`"yes"`，`false`→`"no"` | 636–638 |
| `integer` | `Long.toString` | 640–642 |
| `decimal` | 非有限→`"0"`；整数→整数字符串；否则 `%.2f` 再去尾零、去尾点（`Locale.ROOT`） | 644–652 |
| `registryName` | `id.getPath().toUpperCase(Locale.ROOT)`（**去命名空间**） | 632–634 |
| `itemType` | 空物品→`"AIR"`，否则 `registryName(item key)` | 508–510 |
| `itemName` | 空物品或**无自定义名**→`""`，否则 `getHoverName().getString()` | 512–518 |
| `itemData` | 空物品→`"0"`，否则 `getDamageValue()` | 520–522 |
| `itemDurability` | 空物品→`"0"`，否则 `max(0, getMaxDamage()-getDamageValue())` | 524–526 |
| `emptySlots` | 主背包槽位 0..35 中空槽计数 | 528–536 |
| `totalExperienceAtCurrentLevel` | L≤16: `L²+6L`；L≤31: `2.5L²-40.5L+360`；否则 `4.5L²-162.5L+2220`；再加 `round(progress*xpNeededForNextLevel)` | 538–546 |
| `biome(...,false)` | `toUpperCase(Locale.ROOT)`（如 `PLAINS`） | 548–566 |
| `biome(...,true)` | 以 `_` 分词，首字母大写、其余小写，用空格连接（如 `Plains`、`Dark Forest`） | 548–566 |
| `direction` | 数组 `{"S","SW","W","NW","N","NE","E","SE"}`，索引 `Math.round(yaw/45)&7` | 576–579 |
| `directionXz` | yaw 归一化后 `<=45 或 >=315`→`"+Z"`；`<=135`→`"-X"`；`<=225`→`"-Z"`；否则 `"+X"` | 581–588 |
| `worldType` | `Level.NETHER`→`"Nether"`；`Level.END`→`"The End"`；否则 `"Overworld"` | 590–594 |
| `weatherDuration` | 雷暴取 `getThunderTime()`，否则 `getRainTime()` | 596–605 |
| `worldTime` | `adjusted = floorMod(ticks-6000, 24000)`；`hour = adjusted/1000`；`minute = (adjusted%1000)*60/1000`；24h `%02d:%02d`；12h `h:mm AM/PM`（0 时→12） | 607–618 |
| `coloredPing` | `>100`→`&c`；`>50`→`&e`；否则 `&a`，后接数字 | 620–622 |
| `coloredTps` | `<15`→`&c`；`<18`→`&e`；否则 `&a`，后接 `decimal(tps)` | 624–626 |
| `formatEpoch` | `<=0`→`""`；否则 `yyyy-MM-dd HH:mm:ss`（系统时区） | 58, 628–630 |
| `duration` | 依次输出 `w d h m s`（非零才输出，空格连接）；总秒 `<=0`→`"0s"` | 490–506 |
| `parseId` | 小写；无 `:` 补 `minecraft:` | 420–423 |
| `playerDataTime` | 读 `<world>/playerdata/<uuid>.dat` 的 `creationTime`（first）或 `lastModifiedTime`（last），毫秒；`IOException`→`0` | 425–433 |

### 1.8 本地化（`LanguageService.java`）

* `translatePlaceholderValue`（行 100–116）：仅当
  1) `PlaceholderCatalog.isLocalizable(token)` 为真，且
  2) 值非 null 且**完整匹配**正则 `[A-Za-z]+(?:[ _-][A-Za-z]+)*`（行 26–28）
  时，查 `Placeholder-Translations.<值小写>`，命中则替换。
* 内置可本地化集合 `LOCALIZABLE`（`PlaceholderCatalog.java` 行 42–61，共 18 项）：
  `server_has_whitelist`、`player_allow_flight`、`player_can_pickup_items`、`player_gamemode`、
  `player_has_empty_slot`、`player_has_played_before`、`player_has_health_boost`、`player_online`、
  `player_is_whitelisted`、`player_is_banned`、`player_is_flying`、`player_is_sneaking`、
  `player_is_sprinting`、`player_is_sleeping`、`player_is_inside_vehicle`、`player_is_op`、
  `player_direction`、`player_world_type`。
* `isLocalizable`（行 86–97）额外放行前缀：`server_countdown_`、`player_has_permission_`、`player_has_potioneffect_`。
* 默认词条（`defaults/lang/zh_CN.yml` 行 113–132）：`yes/no`、`SURVIVAL/CREATIVE/ADVENTURE/SPECTATOR`、
  `Overworld/Nether/The End`、`N/NE/E/SE/S/SW/W/NW`、`invalid date`、`invalid format and time`。
* **注意**：`PlaceholderResolver` 调用的是两参重载 `translatePlaceholder(token, value)`（行 102），
  它使用**服务器默认语言**（`selected(defaultLanguage.get())`，行 92），**不是玩家客户端语言**。

### 1.9 `PlaceholderCatalog`

* `SERVER`（行 7–12）：上表 1.3 的 21 个键（不含 `online_*`/`time_*`/`countdown_*` 动态形式）。
* `PLAYER`（行 14–40）：上表 1.6 的全部静态键。
* `supports(token)`（行 66–84）：`server_` / `player_` 前缀 + 静态集合 + 动态前缀白名单。
* **重要**：`supports()` 在运行时代码中**没有任何调用点**（仅 `PlaceholderCatalogTest` 使用）；
  运行时真正生效的只有 `isLocalizable()`。Pumpkin 实现可以不移植 `supports()`。

### 1.10 `ServerMetrics` / `PlayerStatsTracker`

`ServerMetrics.java`（行 6–42）：

* `tick()`：记录 `System.nanoTime()` 到队尾，并丢弃早于 15 分钟的时间戳（行 12–19）。
* `tps(minutes)`：窗口内不足 2 个采样 → `20.0`；否则 `min(20, (count-1)*1e9/(now-first))`（行 21–41）。
* 由 `ChatService` 每 tick 调用（`ChatService.java` 行 509）。

`PlayerStatsTracker.java`（行 9–33）：

* `recordDamage`：按 UUID 存 `(amount, player.tickCount)`（行 13–15）。
* `lastDamage`：无记录→`0.0`（行 17–20）。
* `noDamageTicks`：无记录→`0`，否则 `max(0, 20 - (player.tickCount - record.entityTick))`（行 22–25）。
* 由实体受伤事件写入（`TrChatServerEvents.java` 行 100–102 等）。

### 1.11 PAPI（PlaceholderAPI）跳过清单 ★

**本仓库（NeoForge/Fabric 移植）中不存在任何 PAPI 依赖或调用。**
全仓库仅有 2 处 PAPI 提及，且**都只是文档注释**：

| 位置 | 内容 |
| --- | --- |
| `defaults/channels/Example.yml` 行 1–4 | 指向 `wiki.placeholderapi.com/.../placeholder-list/minecraft/#player` 与 `#server` 的注释链接 |

因此：

| 分类 | 占位符 | 处理建议 |
| --- | --- | --- |
| **PAPI 命名来源，但本仓库已原生实现** | 全部 `%player_*%`（§1.5–1.6）与 `%server_*%`（§1.3–1.4），共约 127 个 token | **必须实现**，不要跳过。命名只是沿用了 PAPI Player/Server 扩展的约定；语义以 `PlaceholderResolver` 为准 |
| **PAPI 风格但需跳过（第三方扩展）** | 任何非 `player_`/`server_` 前缀的 `%xxx%`，如 `%vault_eco_balance%`、`%luckperms_prefix%`、`%essentials_*%`、`%papi_*%` | **不实现**；按 §1.1 规则自然解析为**空字符串** |
| **TrChat 自有本地上下文键（非 PAPI）** | `%trchat_toplayer%`、`%message%`、`%trchat_message_color%` | 必须实现，见 §1.12 |
| **PAPI 独有语法** | `%expansion_identifier%` 的外部注册表、`{placeholder}` 花括号形式、PAPI 的 `%rel_*%`/`%server_online_<player>%` 等扩展专属参数 | **完全不实现**（本仓库无对应代码） |

### 1.12 `local` 上下文键（由 `ChatService` 注入）

| 键（小写） | 值 | 来源 |
| --- | --- | --- |
| `message` | 本条原始消息（trim 后） | `ChatService.java` 行 633 |
| `trchat_message_color` | 玩家可用聊天颜色码 | `ChatService.java` 行 645（常量定义于 `ChannelRenderer.java` 行 25） |
| `trchat_toplayer` | 私聊目标显示名（仅私聊） | `ChatService.java` 行 170 |

**与 `LegacyText.render` 的区别**：`LegacyText.render`（行 35–41）另有一套 `%player%` / `%display_name%` /
`%message%` / `%server%` 的**纯字符串替换**，仅用于日志格式（`settings.yml` 行 2 的注释即指它），
**不经过 `PlaceholderResolver`**，两者不要混淆。

---

## 2. function.yml 规格

### 2.1 文件结构（`defaults/function.yml`）

| 路径 | 含义 | 行号 |
| --- | --- | --- |
| `General.Command-Controller` | 命令拦截规则 | 10–20 |
| `General.Mention` | @ 玩家 | 21–28 |
| `General.Mention-All` | @ 全体 | 29–35 |
| `General.Item-Show` | 物品展示 | 36–44 |
| `General.Inventory-Show` | 背包展示 | 45–50 |
| `General.EnderChest-Show` | 末影箱展示 | 51–56 |
| `Custom.<id>` | 自定义正则功能 | 58–133 |

`Configuration.from`（`ChatFunctionService.java` 行 809–855）只读这 7 个路径；
`Configuration.empty()`（行 802–807）为全部关闭、`customFunctions` 为空的兜底。

### 2.2 `FunctionSettings` 全部键（`FunctionSettings.from`，行 742–758）

| 键 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `Enabled` | bool | `true` | 关闭则该功能完全不收集 token |
| `Permission` | string | `"none"` | `none`（忽略大小写）表示不校验；否则走 `TrChatPermissions.check` |
| `Cooldown` | string | `"0"` | 经 `durationMillis` 解析 |
| `Notify` | bool | `true` | 是否把被提及玩家加入 `mentionedPlayers` |
| `Self-Mention` | bool | `false` | 是否允许 @ 自己 |
| `Pattern` | string | `"@? ?(names)"` | 仅 Mention 使用，`(names)` 为占位符 |
| `Keys` | list | `[]` | 字面触发键（Item/Inventory/EnderChest/Mention-All） |
| `Action` + `Actions` | list | `[]` | **两者合并**，`Action` 在前、`Actions` 在后（行 743–744） |
| `Origin-Name` | bool | `false` | 仅 Item-Show：`true` 用原版物品名（含翻译键），`false` 用 `getHoverName()` |
| `Compatible` | bool | `false` | 仅 Item-Show：`true` 时 hover 用 `new ItemStack(Items.STONE, count)` 替代真实物品 |
| `UI` | bool | `false` | 仅 Item-Show：是否附加"打开容器快照"的点击事件 |

`durationMillis`（行 858–871）：正则 `(?i)(\d+)(ms|s|m|h|d)?` **必须整体匹配**，
无单位视为毫秒，非法输入返回 `0`。

`string(value, fallback)`（行 892–894）：值为字面量 `~` 时等同缺失 → 使用 fallback。

### 2.3 `Custom.<id>` 全部键（行 815–843）

| 键 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `condition` | string | `""` | 经 `ConditionEvaluator.test`；不通过则不收集 |
| `priority` | int | `0` | 仅决定收集顺序（**降序**，行 844） |
| `pattern` | regex | `"(?!)"` | `Pattern.CASE_INSENSITIVE`；非法则整条跳过并告警（行 840–842） |
| `text-filter` | regex | 空 | 可选；`CASE_INSENSITIVE`；取匹配到的**第一段**作为 `{0}` 值 |
| `permission` | string | `"none"` | |
| `cooldown` | string | `"0"` | |
| `action` / `actions` / `Action` / `Actions` | list | `[]` | 四者全部合并（`actions()`，行 884–890） |
| `display.text` | string | `""` | 经 `LegacyText.parse`，`{0}` 替换为匹配值 |
| `display.hover` | string \| list | `""` | list 用 `"\n"` 连接（`multiline`，行 896–901） |
| `display.suggest` | string | `""` | |
| `display.command` | string | `""` | |
| `display.url` | string | `""` | |
| `display.copy` | string | `""` | |

Custom 的 `FunctionSettings` 是硬编码的（行 825–837）：`enabled=true`、`notify=false`、
`selfMention=false`、`pattern=""`、`keys=[]`、`originName=false`、`compatible=false`、`ui=false`。

**display 点击事件优先级**（行 577–606，`else if` 链，只取第一个非空）：
`url` → `command` → `suggest` → `copy`。

* `url`：`{0}` 替换后 trim，**取第一个空格前的子串**，非空且 `new URI(...)` 可解析 → `OPEN_URL`（行 578–587）。
* `command` → `RUN_COMMAND`；`suggest` → `SUGGEST_COMMAND`；`copy` → `COPY_TO_CLIPBOARD`（行 588–606）。

### 2.4 `Command-Controller` 规格

配置解析 `CommandController.from`（行 20–41）：

| 元素 | 规则 | 行号 |
| --- | --- | --- |
| 启用 | `bool(command.get("Enabled"), bool(command.get("Enable"), true))`，即 `Enabled` 优先，其次 `Enable`，默认 `true` | 38 |
| 规则来源 | `List` 的每一项字符串 | 22 |
| 表达式 | 取第一个 `{` **之前**的子串（无 `{` 则整串） | 23 |
| 属性 | 正则 `\{([^}:]+):\s*([^}]*)}`，键 trim + 小写，值 trim | 15, 61–68 |
| `exact` | bool，默认 `false` | 29 |
| `condition` | string，默认 `""` | 30 |
| `cooldown` | 秒（double），`round(value*1000)` ms，默认 `0`；非法 → `0` | 31, 70–76 |
| 正则 | `Pattern.CASE_INSENSITIVE`；语法错误则跳过该条并告警 | 28, 33–35 |

匹配 `CommandController.matching`（行 43–59）：

1. 去掉行首 `/`，trim；空 → `null`。
2. `label = input.split("\\s+", 2)[0]`。
3. 按 `List` **顺序**逐条：
   * `exact == true` → `pattern.matcher(整个输入).matches()`
   * `exact == false` → `pattern.matcher(label).matches()`
4. 首个命中返回。

**默认规则**（`function.yml` 行 17–20）：

| 行 | 原始配置 | 表达式 | 属性 |
| --- | --- | --- | --- |
| 17 | `arasple{exact: true}{condition: perm "trchat.admin"}` | `arasple` | exact=true, condition=`perm "trchat.admin"` |
| 18 | `ver(sion)?(s)?{condition: perm "trchat.admin"}` | `ver(sion)?(s)?` | condition=`perm "trchat.admin"` |
| 19 | `help(s)?{condition: perm *trchat.admin}` | `help(s)?` | condition=`perm *trchat.admin` |
| 20 | `shout{cooldown: 3}` | `shout` | cooldown=3000ms |

`ConditionEvaluator`（`channel/ConditionEvaluator.java` 行 19–47）：空/`~`→true；
`player op` 或 `player is op`→OP 判定；`perm|permission "node"`（引号可选）→权限判定，
前导 `*` 被剥除；前导 `!`→取反；其他→**false**。

**调用顺序（`ChatFunctionService.checkCommand`，行 166–186）**：

1. `commandControllerEnabled() == false` → 直接放行 `true`。
2. 无匹配规则 → 放行 `true`。
3. `rule.condition` 非空且 `ConditionEvaluator.test` 为假 →
   发 `Command-Controller-Deny`，返回 `false`。
4. `rule.cooldownMillis() > 0` **且** 玩家**没有** `trchat.bypass.cmdcooldown`
   **且** `cooldown(player, "command:" + rule.source(), cooldownMillis)` 为假 →
   发 `Command-Controller-Cooldown`，返回 `false`。
5. 否则 `true`。

`isCommandManaged`（行 188–191）：控制器启用 **且** 有匹配规则。用于 `/trchat` 子命令的可用性判断
（`TrChatCommands.java` 行 407–412，未托管时发 `Command-Controller-Disabled`）。

### 2.5 内置功能与 token 优先级（`collectTokens`，行 205–266）

| 功能 | Kind | 优先级 | 键/模式 | 行号 |
| --- | --- | --- | --- | --- |
| Mention-All | `MENTION_ALL` | **600** | `Keys` 字面量，`Pattern.quote` + `CASE_INSENSITIVE` | 209–211, 268–282 |
| Inventory-Show | `INVENTORY` | **550** | 同上 | 212–214 |
| EnderChest-Show | `ENDER_CHEST` | **540** | 同上 | 215–217 |
| Item-Show | `ITEM` | **530** | `Pattern.quote(key) + "-?([1-9])?"`，`CASE_INSENSITIVE`；`group(1)` 为可选槽位号 1–9 | 218–227 |
| Mention | `MENTION` | **500** | 见下 | 228–247 |
| Custom | `CUSTOM` | 配置 `priority` | `Pattern.CASE_INSENSITIVE` | 248–264 |

默认键（`function.yml`）：

* Mention-All（行 34）：`@all`、`@everyone`、`@everybody`、`@所有人`、`@全体成员`
* Item-Show（行 43）：`%i%`、`%i`、`%item%`、`%item`、`[i]`、`[item]`
* Inventory-Show（行 49）：`[inv]`、`[inventory]`
* EnderChest-Show（行 55）：`[ender]`、`[enderchest]`

**Mention 收集细节**（行 228–247）：

1. 在线玩家名列表；`Self-Mention == false` 时排除发送者本人（忽略大小写）。
2. 按**名称长度降序**排序。
3. `alternatives = 各名称 Pattern.quote 后以 "|" 连接`。
4. `configured = config.Pattern().replace("(names)", "(" + alternatives + ")")`。
5. `Pattern.compile(configured, CASE_INSENSITIVE)` 全局匹配；`groupCount() >= 1` 取 `group(1)`，否则取 `group()`。
6. 编译失败仅告警（行 243–245）。

### 2.6 token 接受与去重（`process`，行 94–144）

1. `message = LegacyText.stripLegacyCodes(rawMessage)`（行 95）——`&`/`§` 颜色码在此**已被剥离**。
2. 收集 token；为空 → 直接返回 `literal(message)`、空提及列表、`crossServerSafe = true`（行 101–103）。
3. 排序：`start` **升序**，同 `start` 时 `priority` **降序**（行 104）。
4. 贪心不重叠：仅当 `token.start() >= cursor` 才接受，接受后 `cursor = token.end()`（行 106–112）。
5. 逐 token 输出：先补上 `message[cursor, token.start())` 的纯文本（行 122）。
6. `canUse` 为假 → 原样输出 `message[token.start(), token.end())`（行 123–124）。
7. 否则 `actionKey = kind.name()` 或 `"custom:" + id`；**同一消息内首次出现才执行动作**（行 126–129）。
8. `renderToken`；`Kind.ITEM` 且非空渲染且 `!isVanillaDisplayedItem` → `crossServerSafe = false`（行 131–135）。
9. 渲染为 `null` → 原样输出原文（行 136–138）。
10. 末尾补 `message[cursor, end)`（行 142）。

**注意**：动作（actions）在**渲染之前**执行（行 127–130）。

### 2.7 权限与冷却顺序（`canUse`，行 284–294）

1. `settings.permission()` 不忽略大小写等于 `"none"` 且 `TrChatPermissions.check` 为假 → **拒绝**（不执行动作、不渲染）。
2. `settings.cooldownMillis() <= 0` → 直接通过。
3. `key = kind.name()` 或 `"custom:" + id`。
4. 通过条件：`cooldownGranted.contains(key)` **或** (`cooldown(sender, key, ms)` 为真且加入 `cooldownGranted`)。
   → **同一消息内同 key 只消耗一次冷却**，后续同 key token 免检。

`cooldown`（行 296–318）：

1. 玩家拥有 OP 权限等级 2（`hasPermissions(2)`）→ **直接返回 true 且不写冷却表**（硬编码，无权限节点可配置）。
2. 键 `player.getUUID() + ":" + function`。
3. `until > now` → `false`；否则写 `now + cooldownMillis` 并返回 `true`。

**与命令冷却的差异**：命令冷却用 `trchat.bypass.cmdcooldown` 节点 + OP2 双重放行；
功能冷却只有 OP2 放行。

### 2.8 Action 语法（`runActions`，行 610–641）

**匹配顺序**（先匹配者胜，全部**忽略大小写**）：

| 序 | 形式 | 行为 | 行号 |
| --- | --- | --- | --- |
| 1 | `command "..." as console` / `command "..." as player` | 正则 `(?i)^command\s+['"](.+)['"]\s+as\s+(console\|player)$`；group(1)=命令，group(2)=执行者 | 57–59, 615–617 |
| 2 | `console: <cmd>` | 以控制台身份执行 | 618–619 |
| 3 | `[console] <cmd>` | 同上 | 620–621 |
| 4 | `player: <cmd>` | 以玩家身份执行 | 622–623 |
| 5 | `[player] <cmd>` | 同上 | 624–625 |
| 6 | `message: <text>` | `sender.sendSystemMessage(LegacyText.parse(text.trim()))` | 626–627 |
| 7 | `tell "<text>"` / `tell <text>` | `sender.sendSystemMessage(LegacyText.parse(unquote(text.trim())))` | 628–629 |
| 8 | `sound: <sound>` | 转成控制台命令 `playsound <sound> master <player> ~ ~ ~` | 630–633 |
| 9 | 其他 | 仅记警告日志 `Unsupported function action: ` | 634–635 |

* 每条 action 执行前先做变量替换并 `.trim()`；结果为空则跳过（行 612–613）。
* 任一 action 抛 `RuntimeException` → 捕获并告警，**不中断后续 action**（行 637–639）。

**变量替换 `actionVariables`（行 655–666，按此顺序逐个 `replace`）**：

| 变量 | 替换为 |
| --- | --- |
| `{player}` | 发送者 `getGameProfile().getName()` |
| `%player_name%` | 同上 |
| `{message}` | 已剥离颜色码的整条消息 |
| `{0}` | 该 token 的匹配内容（`Token.argument()`） |

**执行 `executeCommand`（行 643–653）**：`unquote` → 去掉前导 `/` → 空则返回 →
`server.getCommands().performPrefixedCommand(source, command)`；
`player` 用 `sender.createCommandSourceStack()`，其他用 `server.createCommandSourceStack()`。

`unquote`（行 668–675）：仅当首尾同为 `"` 或同为 `'` 且长度 ≥2 时剥去一层。

### 2.9 Mention / Mention-All 渲染

| 功能 | 输出 | 样式 | hover | 行号 |
| --- | --- | --- | --- | --- |
| Mention | `"@" + 目标真实名` | AQUA | `Function-Mention-Hover`（`{0}`=发送者名，`{1}`=目标名） | 331–350 |
| Mention-All | 字面量 `"@所有人"`（**硬编码，不随语言变化**） | GOLD + BOLD | `Function-Mention-All-Hover`（`{0}`=发送者名） | 352–372 |

* Mention 目标离线 → 原样返回匹配文本（行 333）。
* `Notify == true` 时才把目标加入 `mentionedPlayers`（行 335–337, 353–359）。
* Mention-All 的 Notify 会把**除发送者外所有在线玩家**加入提及列表（行 354–358）。
* 原文中的 `@` 可有可无：`Pattern` 的 `@?` 不参与替换，输出**总是**带 `@`。

`notifyMention`（行 375–399，由 `ChatService` 只对真正收到消息的被提及者调用）：

1. 音效 `SoundEvents.ANVIL_LAND`，音量 `1.0`，音调 `2.0`。
2. 动作栏消息 `Function-Mention-Notify`（`{0}`=发送者名），`true` = 动作栏。
3. 标题动画 `ClientboundSetTitlesAnimationPacket(10, 50, 10)`。
4. 主标题 `Function-Mention-Title`（`{0}`=发送者名）。
5. 副标题 `Function-Mention-Subtitle`（`{0}`=发送者名）。

### 2.10 Item-Show 实现（`item`，行 401–446）

1. 槽位：`argument` 为空 → 当前选中快捷栏槽；否则 `parseInt(argument) - 1`（行 402–406）。
2. 物品为空 → `Function-Item-Air` 组件 + GRAY 样式（行 408–410）。
3. `hoverStack = settings.compatible() ? new ItemStack(Items.STONE, count) : stack`（行 411）。
4. 名称：`Origin-Name == true` → `stack.getItemName()`（≥1.21.11）/ `Component.translatable(getDescriptionId())`；
   否则 → `stack.getHoverName()`（行 412–418）。
5. 文本 = `"[" + 名称 + " x" + count + "]"`，AQUA（行 419–423）。
6. hover = `HoverEvent.ShowItem(hoverStack)`（行 424–432）。
7. `UI == true` → 追加 `ClickEvent.RunCommand("/trchat view " + snapshotId)`（行 433–444）。

**UI 开关含义**：`UI: true` 让物品文本可点击打开一个 3×9 只读容器快照；
`UI: false` 只显示文本 + 物品悬浮，无点击行为。默认 `function.yml` 行 42 为 `true`。

`createItemSnapshot`（行 515–558）：

* id = 随机 UUID 去 `-` 后取前 **12** 位十六进制（行 517）。
* 容器内容：≥1.20.5 读 `DataComponents.CONTAINER`（最多 27 项，行 519–533）；
  旧版读 NBT `Items`（行 534–544）。
* 内容为空 → 放 **13 个 `ItemStack.EMPTY`**，再把物品本身放在索引 13（3×9 的正中，行 545–548）。
* `size = 27`；标题 `Function-Item-Title`（`{0}`=玩家名，`{1}`=`stack.getHoverName().getString()`，行 549–556）。
* **不执行 100 条上限裁剪**（对比 `createSnapshot`）。

`isVanillaDisplayedItem`（行 448–460）：槽位物品非空且其命名空间满足
`TrChatProtocol.isCrossServerSafeItemNamespace`（实现为 `"minecraft".equals(namespace)`，
`protocol/TrChatProtocol.java` 行 21–23）→ 视为跨服安全。

### 2.11 Inventory-Show / EnderChest-Show 实现（`inventory`，行 462–482；`createSnapshot`，行 484–513）

| 项 | 背包 | 末影箱 |
| --- | --- | --- |
| 容器尺寸 | **54** | **27** |
| 内容 | 主背包槽 0–35，再依次追加副手、头盔、胸甲、护腿、靴子（索引 36–40） | 末影箱槽 0–26 |
| 文本 | `Function-Inventory-Format`（`{0}`=玩家名），AQUA | `Function-EnderChest-Format`（`{0}`=玩家名），AQUA |
| hover | `Function-Inventory-Hover`（**无参数**）的 `SHOW_TEXT` | `Function-EnderChest-Hover`（**无参数**）的 `SHOW_TEXT` |
| click | `RUN_COMMAND "/trchat view <id>"` | 同左 |
| 标题 | `Function-Inventory-Title`（`{0}`=玩家名） | `Function-EnderChest-Title`（`{0}`=玩家名） |

**渲染方式**：hover **不是**物品列表，而是一行**文本提示**；真实内容通过点击后由服务器
打开一个只读 9×N 容器界面呈现（"快照"模式）。

快照生命周期：

* id = 12 位十六进制；存入 `LinkedHashMap<String, Snapshot>`（行 486, 508）。
* **TTL = 5 分钟**（`SNAPSHOT_TTL`，行 56）；`expireSnapshots`（行 677–680）在创建与打开时惰性清理。
* 上限 **100** 条，超出时移除**最旧插入**的一条（行 509–511）。
* `openSnapshot`（行 146–164）：过期/不存在 → 发 `Function-Snapshot-Expired` 并返回 false；
  否则以 `SimpleContainer` + `ReadOnlyChestMenu` 打开，标题为快照标题。
* 入口命令 `/trchat view <snapshot>`（`TrChatCommands.java` 行 231–236, 729）。

`ReadOnlyChestMenu`（行 16–58）：

* `create`：`size == 54` → `MenuType.GENERIC_9x6`（6 行）；否则 `GENERIC_9x3`（3 行）。行 28–33。
* `clicked(...)` → 只调用 `broadcastFullState()`，不做任何移动。行 35–42。
* `quickMoveStack` → `ItemStack.EMPTY`。行 44–47。
* `canDragTo` → `false`；`canTakeItemForPickAll` → `false`。行 49–57。

### 2.12 语言键汇总（`defaults/lang/*.yml`）

| 键 | 用途 |
| --- | --- |
| `Command-Controller-Disabled` | 命令未托管 |
| `Command-Controller-Deny` | condition 不通过 |
| `Command-Controller-Cooldown` | 命令冷却中 |
| `Function-Snapshot-Expired` | 快照过期 |
| `Filter-Anvil-Blocked` | 铁砧名含敏感词 |
| `Function-Mention-Notify` / `-Title` / `-Subtitle` / `-Hover` / `-All-Hover` | 提及提示 |
| `Function-Item-Air` / `-Title` | 空手 / 物品快照标题 |
| `Function-Inventory-Format` / `-Hover` / `-Title` | 背包 |
| `Function-EnderChest-Format` / `-Hover` / `-Title` | 末影箱 |

（zh_CN 行 52, 97–111, 133–142；en_US 同键名。）

---

## 3. filter.yml 规格

### 3.1 全部键语义（`FilterService.Settings.from`，行 257–277）

| 键 | 类型 | 默认值 | 语义 | 行号 |
| --- | --- | --- | --- | --- |
| `Enable.Chat` | bool | `true` | 聊天消息过滤开关 | 266 |
| `Enable.Sign` | bool | `true` | 告示牌周期性过滤开关 | 267 |
| `Enable.Anvil` | bool | `true` | 铁砧命名过滤开关 | 268 |
| `Cloud-Thesaurus.Enabled` | bool | `true` | 云端词库开关 | 269 |
| `Cloud-Thesaurus.Urls` | list | `[]` | 云端词库 JSON 地址 | 270 |
| `Cloud-Thesaurus.Ignored` | list | `[]` | 忽略词（**统一小写**后成集合） | 271 |
| `Local` | list | `[]` | 本地敏感词 | 272 |
| `Ignored-Punctuations` | list | `[]` | 每项**逐字符**展开、逐字符 `toLowerCase`，成 `Set<Character>` | 260–263, 273 |
| `WhiteList` | list | `[]` | 白名单短语 | 274 |
| `Replacement` | string | `"*"` | 空串 → `'*'`；否则**只取首字符** | 264, 275 |

默认值（`defaults/filter.yml`）：`Chat/Sign/Anvil = true`；`Cloud-Thesaurus.Enabled = true`；
`Ignored = ['nt']`；`Urls = ['https://raw.githubusercontent.com/Yurinann/Filter-Thesaurus-Cloud/main/database.json']`；
`Local = ['NMSL','fuck','shit']`；`WhiteList = ['has been']`；`Replacement = '*'`；
`Ignored-Punctuations` 为 50 项（行 17），含半角 `!.,#$%&*()|?/@";[]{} +~-_=^<>`、空格、
全角 `　！。，￥（）？、“‘；【】——……《》`、反斜杠与反引号。

**注意**：`Settings.empty()`（行 253–255）把所有开关置 `false`、`Replacement='*'`，
即**加载失败时不进行任何过滤**。

### 3.2 三个触发点

| 入口 | 逻辑 | 行号 |
| --- | --- | --- |
| `filterChat(player, input)` | `!Chat` 或玩家 OP2 → 原样返回；否则 `filter(input).text()` | 75–86 |
| `checkAnvil(player, name)` | `!Anvil` 或 `name == null` 或 OP2 → `true`；`matches() == 0` → `true`；否则发 `Filter-Anvil-Blocked` 并 `false` | 93–107 |
| `tick()` | 每 tick `ticks++`；`Sign` 且 `ticks % 20 == 0` → 遍历已加载区块的方块实体，对每个 `SignBlockEntity` 调用 `filterSign`；`ticks >= 72000` → 归零并 `refreshCloudAsync()`（≈1 小时） | 117–132 |

`chunkLoaded` / `chunkUnloaded`（行 109–115）维护 `Set<LevelChunk>`。

`filterSign`（行 134–173）：

* 正面 + 背面各 4 行：`filter(message.getString())`；`matches() > 0` 时把该行替换为
  `Component.literal(result.text())` 并**保留原 style**。
* 有改动且 `sign.getLevel() != null` → `sendBlockUpdated(pos, state, state, 3)`（行 170–172）。

调用点：`ChatService.java` 行 745（聊天）、`TrChatServerEvents*.java` 行 119/127（铁砧）。

### 3.3 `TextFilter` 匹配算法（`TextFilter.java` 行 13–53）

**预处理**

1. `input == null || input.isEmpty() || sensitiveWords.isEmpty()` → 原样返回，`matches = 0`（行 20–22）。
2. `lower = input.toLowerCase(Locale.ROOT)`（行 24）。
3. `protectedCharacters[]`：对每条白名单（**小写**）用 `indexOf` 找全部出现位置并标记为受保护（行 80–94）。
4. 敏感词：去 null/空白 → 小写 → `distinct()` → **按长度降序**排序（行 28–33）。

**主循环**（行 35–51）

* `start` 从 0 到 `len-1`：
  * 若 `protectedCharacters[start]` 或 `normalize(output[start])` ∈ `Ignored-Punctuations` → `continue`。
  * 按长度降序逐个尝试敏感词；首个匹配即：
    * 把 `output[start .. match.end()]` 中**不在标点集合里**的字符替换为 `replacement`；
    * `matches++`；`start = match.end()`；`break`。
* 返回 `Result(新字符串, matches)`。

**单词匹配 `match`**（行 55–78）

* 从 `start` 起同时推进输入索引与词索引。
* 输入索引落在受保护字符 → **立即返回 null**（白名单可阻止跨词匹配）。
* 输入字符若 `normalize` 后属于 `Ignored-Punctuations` → **跳过该字符（不推进词索引）**。
* 否则与词字符 `normalize` 后比较，不等 → `null`。
* 词全部消费完 → 返回 `Match(inputIndex - 1)`；否则 `null`。

**字符归一化 `normalize`**（行 96–104）——**这是唯一的"变体"处理**：

| 输入 | 输出 |
| --- | --- |
| `12288`（U+3000 全角空格） | `' '` |
| `65281..65374`（U+FF01–U+FF5E 全角 ASCII） | `value - 65248`（转半角） |
| 其他 | `Character.toLowerCase(value)` |

**结论（重要）**：

* ✅ **忽略大小写**（全程 `toLowerCase`）。
* ✅ **忽略标点/空白**：仅限 `Ignored-Punctuations` 中列出的字符；**配置里没有的字符不会被跳过**。
  默认配置含半/全角空格，因此 `f u c k`、`f.u,c-k` 均可命中。
* ✅ **全角→半角折叠**（U+FF01–FF5E 与 U+3000）。
* ❌ **不支持同音字 / 拼音 / 形近字 / leet 替换 / 变体映射**（无任何映射表）。
* ✅ **白名单**：纯子串（忽略大小写）保护，且被保护的区间**不能**成为任何匹配的起点或跨越点。
* ✅ **替换保留标点**：只有被消费的敏感字符被替换，被跳过的标点原样保留。

**行为证据（`src/test/java/.../filter/TextFilterTest.java`）**

| 用例 | 输入 | 词表 | 标点 | 白名单 | 期望输出 | 期望 matches |
| --- | --- | --- | --- | --- | --- | --- |
| 行 13–23 | `This F.u_C-k is hidden` | `[fuck]` | `{'.','_','-'}` | `[]` | `This *.*_*-* is hidden` | 1 |
| 行 25–35 | `has been and has` | `[has]` | `{' '}` | `[has been]` | `has been and ***` | 1 |

### 3.4 `Cloud-Thesaurus` 加载（`refreshCloudAsync` 行 175–203；`fetch` 行 205–239）

1. `!cloudEnabled` 或 `urls` 为空 → 直接返回（保留 `words = localWords`）。
2. 后台线程（≥1.20.5 用虚拟线程，名字 `TrChat-Filter-Cloud`）：
   `collected = new HashSet<>(localWords)`，再对每个 url `addAll(fetch(url, cloudIgnored))`。
3. `words = collected` 按**长度降序**排序后的不可变列表（行 184–187）。
4. 缓存文件：`<config>/trchat/filters/<hex(url.hashCode())>.json`（行 206）。
5. HTTP：GET，连接与请求超时均 30 秒，UTF-8 读字符串；成功后写入缓存（行 209–217）。
6. 请求失败 → 回读缓存文件；仍失败 → 返回空列表（行 218–225）。
7. 解析：根对象 `words` 数组，逐项 `getAsString()`；`toLowerCase` 后命中 `Ignored` 集合则丢弃（行 226–234）。
8. 任何 `RuntimeException` → 告警 `Invalid cloud thesaurus from <url>`，返回空列表（行 235–238）。

**注意**：`reload()`（行 59–73）先 `words = settings.localWords()`，随后异步刷新；
若云端尚未返回，只有本地词生效。云端失败时**本地词仍然生效**（因为 HashSet 以 localWords 起始）。

**Pumpkin 支持情况**：已按本节移植到 `pumpkin/src/cloud.rs`（`http.rs` 提供共用的 `wasi:http` GET，30 秒超时）。

- `Cloud-Thesaurus.*` 三个键全部接入（`config.rs` 的 `FilterConfig`，已去掉 `#[allow(dead_code)]`）。
- 刷新跑在宿主调度器上：插件加载时排程 `delay = 1 tick`、`period = 72_000 ticks`（一小时）——与 Mod 的
  `submitAsync(period = 60 * 60 * 20)`（taboolib 以 **tick** 计）及本地重实现的 `ticks >= 72_000` 一致；
  另外 `/trchat reload` 成功后的下一 tick 会立刻再刷新一次（对应 `loadFilter(updateCloud = true)`）。
  首次抓取因此不阻塞插件加载，重载也不会被网络拖住。
- 词表按 Mod 语义**跨刷新累加**（本地重实现是每次整体替换），排序按长度降序并去重；`words` 逐项减去
  `Ignored`（比较统一转小写）；`lastUpdateDate` 与上次**相同**的库视为已应用（`readDatabase` 行 137-141）。
- 抓取失败回读 `{data_folder}/filters/<hex(url.hashCode())>.json` 缓存（本地重实现的缓存命名）；
  缓存也不可用时只记警告，不抛错。
- 播报走插件日志（对应 Mod 的 `console().sendLang`）：加载时 `Plugin-Loaded-Filter-Local`（本地词数）、
  成功 `Plugin-Loaded-Filter-Cloud`（词数 / url / `lastUpdateDate`）、累计词表仍为空才
  `Plugin-Failed-Load-Filter-Cloud`。
- `filter::text_filter` 把本地词表与云端词表合并后同时供聊天、告示牌、铁砧三条管线使用，
  因此“云端失败时本地词仍然生效”的第 20 条结论依旧成立，沙盒内无网络时亦同。

---

## 4. 关键结论与实现坑（≤20 行）

1. 占位符语法 `%([^%]+)%`，token 先 `trim().toLowerCase()`；**未知占位符解析为空串并被删除**，不是原样保留。
2. `viewer` 参数在 `PlaceholderResolver` 中**从未使用**；所有 `player_*` 一律以消息主体为准。
3. `PlaceholderCatalog.supports()` **无运行时调用点**，只有 `isLocalizable()` 参与本地化。
4. 本地化用的是**服务器默认语言**，不是玩家客户端语言（`resolve` 调用两参 `translatePlaceholder`）。
5. `server_tps` 与 `tps_1/5/15` 来源不同：前者用原版平均 tick 时间，后者用 `ServerMetrics` 滑动窗口。
6. `player_time_offset`、`player_max_no_damage_ticks`、`player_online` 是**常量**（`"0"`/`"20"`/`"yes"`）。
7. 功能 token 优先级：Mention-All 600 > Inventory 550 > EnderChest 540 > Item 530 > Mention 500 > Custom(配置值)；
   同起点按优先级降序，整体贪心不重叠。
8. **动作在渲染之前执行**，且同一消息内同 `actionKey`（kind 名或 `custom:<id>`）**只执行一次**。
9. 功能冷却顺序：权限 → 冷却；**OP2 硬编码放行且不写冷却表**，无权限节点可配置。
10. 命令冷却顺序：控制器启用 → 命中规则 → `condition` → `trchat.bypass.cmdcooldown` → 冷却；
    `Command-Controller.Enabled` 优先于 `Enable`。
11. `Command-Controller.List` 表达式取**第一个 `{` 之前**的子串；`exact:false` 时只匹配命令 **label**。
12. `sound:` 实际拼成 `playsound <原样文本> master <player> ~ ~ ~`——文档注释里的 `[volume] [pitch]` **不被解析**，不要照抄。
13. `Mention-All` 输出硬编码 `"@所有人"`，且 Mention 输出**总是补 `@`**（即使原文没有）。
14. Item/Inventory/EnderChest 的 hover 是**文本提示**，真实内容靠点击 `RUN_COMMAND /trchat view <id>`
    打开只读 9×N 容器；快照 TTL 5 分钟、上限 100 条。
15. `UI: false` 时 Item-Show **没有**点击事件；`Compatible: true` 会把 hover 物品换成同数量石头；
    `Origin-Name: true` 用原版物品名而非自定义名。
16. `createItemSnapshot` 在容器为空时放 13 个空槽 + 物品在索引 13，且**不受 100 条上限约束**。
17. `TextFilter` 只做**大小写折叠 + 全角→半角 + 跳过配置标点**；**没有同音字/拼音/形近字/leet 支持**。
18. `Ignored-Punctuations` 是**逐字符**展开的 `Set<Character>`；不在表里的字符会阻断匹配（默认含半/全角空格）。
19. 白名单是忽略大小写的**纯子串**保护，且被保护区间不能作为匹配起点或跨越点。
20. `filter.yml` 加载失败时 `Settings.empty()` 全关 = **完全不过滤**；云端词库失败时本地词仍然生效。

### PAPI 跳过结论（一句话）

本移植**没有任何 PAPI 代码**；`player_*` / `server_*` 命名只是沿用 PAPI 约定且已原生实现（**必须实现**），
而 `%vault_*%`、`%luckperms_*%`、`%essentials_*%` 等第三方扩展占位符按规则解析为**空字符串**（**跳过**）。
