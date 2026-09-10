<!-- 
感谢您为 Velowork 贡献代码！
提交 PR 前请确认：
1. PR 标题严格遵循 Conventional Commits：<type>(<scope>): <subject>
   示例：fix(chrome): enable custom titlebar on Windows platform
2. 关联对应的 Issue（如 Closes #123）
-->

## 变更概述 (Summary)

<!-- 请简明扼要地描述此 PR 解决了什么问题或引入了什么功能 -->

## 关联 Issue (Related Issues)

- Closes #
- Fixes #

## 变更类型 (Type of Change)

- [ ] `fix`: 缺陷修复
- [ ] `feat`: 新增功能/特性
- [ ] `refactor`: 重构优化（无功能与行为变动）
- [ ] `perf`: 性能调优
- [ ] `test`: 测试用例增补
- [ ] `docs`: 文档/注释变动
- [ ] `ci`: CI 流水线或打包脚本变动
- [ ] `chore`: 杂项维护/版本更新

## 涉及模块 (Scope)

- [ ] `velowork-app`
- [ ] `velowork-ui`
- [ ] `velowork-terminal`
- [ ] `velowork-workspace`
- [ ] `velowork-core` / `velowork-security`
- [ ] Other: 

## 自检清单 (Pre-Merge Checklist)

- [ ] **CI 验证**：本地运行 `cargo check`、`cargo clippy --all-targets -- -D warnings` 与 `cargo test` 均通过
- [ ] **国际化覆盖**：所有新增界面可见文本均已接入 `i18n!` 宏，无硬编码中文/英文
- [ ] **稳定性**：生产运行路径零裸 `unwrap()` / `expect()` 隐患
- [ ] **防穿透**：浮动层、弹窗、下拉菜单最外层均显式包含 `.occlude()` 与 `cx.stop_propagation()`
- [ ] **DCO 签名**：所有 Commit 均包含 `Signed-off-by` (通过 `git commit -s` 提交)
