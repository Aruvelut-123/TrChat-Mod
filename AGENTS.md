# 仓库协作规范

- 开始任何仓库任务时，必须先执行 `git fetch origin v2`，确认用户远程仓库的最新提交后再进行检查、编辑或提交；如果获取失败，必须先向用户报告。
- 获取 `origin/v2` 后，必须继续执行 `git fetch upstream v2`，并将 `upstream/v2` 与 `upstream-sync.properties` 记录的 Bukkit 上游基线比较；如果上游有新提交，必须先审计并移植适用于本项目的变更，更新同步基线并通过测试；完成移植后，还必须以保留工作树的同步合并将 `upstream/v2` 记录为当前分支的祖先，确保 GitHub 不显示落后上游，然后再处理当前任务。
- 本项目是 TrChat Bukkit 插件的多加载器（NeoForge + Fabric）多版本（1.21.1、1.21.11、26.1.2、26.2）重实现，使用 Stonecutter 管理版本差异：
  - `versions/<mc>-<loader>/` 目录中的每个节点共享 `src/main` 源码，通过 `//? if`、`//? if >=版本` 条件注释切换。
  - loader 特有代码（入口点、事件绑定、配置、权限）放在独立文件并用 `//? if neoforge` / `//? if fabric` 包裹整个文件。
  - loader 无关的命令逻辑放在 `TrChatCommands`（NeoForge 事件在 `TrChatServerEvents`，Fabric 在 `TrChatServerEventsFabric`）。
  - 修改共享源码后必须分别验证 8 个节点（4 版本 × 2 加载器）的 `:版本-loader:test` 编译与测试通过。
- 所有 Git 提交标题必须使用 Conventional Commits 格式：`<type>(<scope>): <中文描述>`。
- `type` 与可选的 `scope` 保持英文小写，以兼容 CI；冒号后的提交描述必须使用中文。
- 每个提交只包含一个逻辑改动，并在提交前运行与改动风险相符的测试。
- 不要提交或覆盖与当前任务无关的工作区改动。
- 模组版本号的前三段必须与上游版本一致；仅在用户明确允许更新版本号时，递增本项目的第四段版本号。
