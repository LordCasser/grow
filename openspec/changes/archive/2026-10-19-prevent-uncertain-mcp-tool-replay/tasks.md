## 1. Audit and contract

- [x] 1.1 核对普通工具、direct 入口、旧重试分支和现有取消/实例保护契约，确认无未发送证据。
- [x] 1.2 建立不自动重放的最小 delta 与设计，不修改 MRTR 范围。

## 2. Implementation and verification

- [x] 2.1 移除失败调用的自动重发，仅保留原实例条件重置和下一次独立初始化。
- [x] 2.2 明确传输丢失/timeout 的结果未知说明，保留业务错误与原 request/peer 取消语义。
- [x] 2.3 更新旧重试测试，运行写入后响应丢失、下一次独立调用、替换、timeout/取消回归。
- [x] 2.4 更新开发者说明与版本记录，完成所需 crate/格式、OpenSpec strict 校验；核对证据后归档。
