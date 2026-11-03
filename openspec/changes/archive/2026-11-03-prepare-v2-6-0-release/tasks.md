## Review and alignment

- [x] 审查候选改动与契约、prompt、调用方，记录结论和覆盖边界。
- [x] 对齐当前工具接口、会话操作的 prompt 和文档，核对本地引用。
- [x] 同步 2.6.0 版本与 Cargo.lock，编写发布说明。

## Verification

- [x] 使用指定工具链执行适用静态检查、回归和生产构建，记录实际结果。
- [x] 对照审查结论检查失败路径与发布限制，不把未验证事项写成通过。
- [x] 完成 strict 全量验证并归档，执行 strict 全量和 archive 校验。

这些任务只覆盖候选准备和审查交付；MCP/watcher/cold-load 的发布阻塞仍见 review.md 与 backlog，未标为修复。未提交、打 tag 或发布。
