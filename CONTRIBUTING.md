# 贡献与提交规范

本仓库后续提交统一使用 [Conventional Commits](https://www.conventionalcommits.org/)：

```text
<type>(<scope>): <description>
```

`scope` 可省略；存在不兼容改动时，在类型或作用域后添加 `!`。标题应简洁描述单一改动，最多 100 个字符。

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
feat(channels): add trade channel support
fix(data): persist active channel across restarts
docs(config): explain Auto-Join uniqueness
chore(release): bump version to 2.4.9.2
```

合并提交由 GitHub 自动生成，可以使用标准 `Merge ...` 标题。CI 会检查每次 Push 或 Pull Request 新增的提交；本地可运行：

```powershell
python scripts/check-commit-messages.py HEAD~1..HEAD
```

All future commits must follow Conventional Commits. Use one of the types listed above, an optional lowercase scope, and a concise description.
