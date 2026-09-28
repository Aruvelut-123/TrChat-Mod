# 贡献与提交规范

本仓库后续提交统一使用 [Conventional Commits](https://www.conventionalcommits.org/)：

```text
<type>(<scope>): <description>
```

`scope` 可省略；存在不兼容改动时，在类型或作用域后添加 `!`。标题应简洁描述单一改动，最多 100 个字符。`type` 与可选的 `scope` 使用英文小写，冒号后的描述统一使用中文。

允许的 `type`：

- `feat`：新增功能
- `fix`：问题修复
- `docs`：仅文档
- `refactor`：不改变外部行为的重构
- `perf`：性能优化
- `test`：测试
- `build`：构建系统或依赖
- `ci`：持续集成
- `chore`：维护、版本发布等杂项
- `revert`：回滚提交

常用 `scope` 示例：`channels`、`commands`、`data`、`redis`、`release`。

```text
feat(channels): 添加交易频道支持
fix(data): 持久化玩家当前频道
docs(config): 说明 Auto-Join 唯一性限制
chore(release): 更新版本至 2.4.9.2
```

合并提交由 GitHub 自动生成，可以使用标准 `Merge ...` 标题。CI 会检查每次 Push 或 Pull Request 新增的提交；本地可运行：

```powershell
python scripts/check-commit-messages.py HEAD~1..HEAD
```

All future commits must follow Conventional Commits. Keep the type and optional scope lowercase, and write the description after the colon in Chinese.
