## 1. 前置与矩阵

- [x] 1.1 核对三个前置 change 的实现/验证/归档记录，将所测代码版本及离线工具版本写入 `verification.md`。
- [x] 1.2 建立 `route-matrix.md`，核对当日官方缓存契约和本地可见 route 元数据；模型别名、部署身份、字段透传无法确认的行明确标记未知与未执行。
- [x] 1.3 在本目录建立合成 fixtures 与有界实验脚本，为每个 route 固定请求/输出/费用预算；先用 mock 验证预算停止、证据记录与输入中没有真实会话数据。

## 2. 理论论证与 review

- [x] 2.1 审阅主请求 key、三个 wire 后端的缓存参数、Messages breakpoint 与 recap Sideband 的代码和现有测试，逐处区分本地断言与远端命中推断。
- [x] 2.2 对照官方文档逐项推导图片追加、tools/settings、epoch/key、Messages 长尾与 breakpoint、tool-free recap、TTL/并发的适用边界；更新 route 矩阵，不把未知模型/网关当成已验证 route。
- [x] 2.3 在 `results.md` 给 key 生命周期、breakpoint、retention 候选写保留现状、另开行为 change 或证据不足的结论，附官方来源、代码依据和缺失的实测条件；不得写命中率、延迟或费用改善数字。
- [x] 2.4 修正 recap 缓存注释和测试名称，把需要真实 route 验证的策略边界登记到 backlog；不改变生产缓存行为。

## 3. 验证与归档

- [x] 3.1 在 `verification.md` 记录理论 review 的代码/官方来源、mock 结果、未发送请求与未量化结论的边界，核对引用和本次验收范围。
- [x] 3.2 通过 `openspec validate --all --strict --no-interactive`，交付理论结论后以 `--skip-specs` 归档，再执行全量规范及 `openspec validate --archived --no-interactive`。
