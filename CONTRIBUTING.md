# 贡献指南

感谢您对 ChainFusion Arbitrage 项目的关注！我们欢迎各种形式的贡献。

## 如何贡献

### 报告 Bug

如果您发现了 Bug，请通过 [Issues](https://github.com/your-username/chainfusion-arbitrage/issues) 提交，并包含以下信息：

- 操作系统和版本
- Rust 版本 (`rustc --version`)
- 完整的错误信息和日志
- 重现步骤
- 期望的行为

### 功能建议

欢迎提出新功能建议！请创建一个 Issue 并：

- 清晰描述您想要的功能
- 解释为什么这个功能对项目有价值
- 如果可能，提供实现思路

### 提交代码

1. **Fork 仓库**

   点击页面右上角的 Fork 按钮

2. **克隆到本地**

   ```bash
   git clone https://github.com/your-username/chainfusion-arbitrage.git
   cd chainfusion-arbitrage
   ```

3. **创建分支**

   ```bash
   git checkout -b feature/your-feature-name
   # 或
   git checkout -b fix/bug-description
   ```

4. **编写代码**

   - 遵循现有的代码风格
   - 添加必要的测试
   - 更新相关文档

5. **提交更改**

   ```bash
   git add .
   git commit -m "feat: add new feature description"
   ```

6. **推送到 GitHub**

   ```bash
   git push origin feature/your-feature-name
   ```

7. **创建 Pull Request**

   - 填写清晰的 PR 描述
   - 关联相关的 Issue

## 代码规范

### Rust 代码

- 使用 `cargo fmt` 格式化代码
- 使用 `cargo clippy` 检查代码质量
- 遵循 [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)

```bash
# 格式化
cargo fmt

# Lint 检查
cargo clippy -- -D warnings

# 运行测试
cargo test
```

### 提交信息规范

使用 [Conventional Commits](https://www.conventionalcommits.org/) 格式：

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

类型 (type)：

| 类型 | 说明 |
|------|------|
| feat | 新功能 |
| fix | Bug 修复 |
| docs | 文档更新 |
| style | 代码格式（不影响功能） |
| refactor | 重构 |
| perf | 性能优化 |
| test | 测试相关 |
| chore | 构建/工具相关 |

示例：

```
feat(executor): add Flashbots bundle support

- Implement Flashbots client
- Add bundle building logic
- Support multiple block targets

Closes #123
```

### 文档

- 为公共 API 添加文档注释
- 更新 README 和相关文档
- 中英文文档保持同步

## 开发环境

### 安装依赖

```bash
# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 安装开发工具
rustup component add rustfmt clippy
```

### 运行测试

```bash
# 运行所有测试
cargo test

# 运行特定模块测试
cargo test -p strategies

# 运行集成测试
cargo test --test integration
```

### 本地调试

```bash
# 开启详细日志
RUST_LOG=debug cargo run -p main
```

## Pull Request 检查清单

提交 PR 前，请确保：

- [ ] 代码通过 `cargo fmt` 格式化
- [ ] 代码通过 `cargo clippy` 检查
- [ ] 所有测试通过
- [ ] 添加了必要的测试
- [ ] 更新了相关文档
- [ ] 提交信息符合规范

## 代码审查

所有 PR 都需要经过代码审查。审查者会关注：

- 代码质量和可读性
- 测试覆盖率
- 性能影响
- 安全考量
- 文档完整性

## 许可证

您的贡献将采用与项目相同的 [MIT 许可证](LICENSE)。

## 联系方式

如有问题，请通过以下方式联系：

- 提交 [Issue](https://github.com/your-username/chainfusion-arbitrage/issues)
- 发送邮件至 your-email@example.com

再次感谢您的贡献！
