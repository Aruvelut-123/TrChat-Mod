# TrChat for PumpkinMC（实验性）

> ⚠️ **实验性（Experimental）**：本目录是 TrChat v2 对
> [PumpkinMC](https://pumpkinmc.org)（Rust 实现的 Minecraft 服务器）的独立实验性移植，
> 处于 **WIP** 状态，功能为最小可用核心，不保证生产可用。

## 这是什么

TrChat 是一个多平台聊天插件（Bukkit / Bungee / Velocity）。PumpkinMC 的插件机制与
Bukkit 完全不同——插件是 **WASM Component**（Rust / Go / Kotlin 均可，本移植使用
**Rust**，Pumpkin 官方首选语言）。因此本目录是一个 **独立 Rust crate**，不属于
Gradle 构建，与 `project/` 下的 Kotlin 模块并存。

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
目录即被加载。GitHub Actions 构建产物（`trchat-pumpkin` artifact）也可直接使用。

## 功能（当前）

* 拦截 `PlayerChatEvent`（最高优先级、阻塞模式）
* 按 `config.json` 中的 `format` 模板渲染聊天消息（支持 `{player}` / `{message}` 占位符）
* 将渲染结果广播给所有在线玩家，并抑制服务器默认聊天
* 声明了 Redis 互通所需的全部网络权限（`network.tcp.*`、`network.dns`、`network.loopback`）

## 配置

首次启动会在 `plugins/data/trchat/config.json` 生成默认配置：

```json
{
  "format": "&7<&f{player}&7> &f{message}",
  "redis_enabled": false,
  "redis_url": "redis://127.0.0.1:6379/"
}
```

## Roadmap（实验阶段后续）

- [ ] 频道系统：前缀路由（`#global` / `@local`），对齐 Bukkit 版 `Channel` 语义
- [ ] 私聊命令 `/msg`（`PlayerCommandPreprocessEvent` 拦截）
- [ ] Redis 跨服互通：监听 `trchat-message` 频道，与现有 Bukkit/Bungee/Velocity 聊天体系打通
  （需要 `network.tcp.connect` 权限 —— 已在本插件 metadata 中声明；WASI 沙箱内 TCP 行为需编译验证）
- [ ] 权限节点注册（`trchat.command.channel.*` 等）

## 说明

* Pumpkin 插件 API 版本锁定为 git commit `fc780ec`（master，对应 `0.1.0-dev+26.2-26.45` 协议族）。
* 事件、命令、权限等 WIT 定义位于
  `crates/pumpkin-plugin-wit/v0.1/`（[Pumpkin-MC/Pumpkin](https://github.com/Pumpkin-MC/Pumpkin) 仓库内）。
