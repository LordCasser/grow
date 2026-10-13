## 1. Proposal

- [x] 1.1 核对固定 Grow/upstream 基线、现有规范及 working tree 差异，形成 proposal/design/spec delta。
- [x] 1.2 完成严格 OpenSpec 文档校验并记录 verification。

## 2. Product decisions and backend — SDK stage in progress

- [x] 2.0 核查并选定官方 rmcp 3.5.1；产出 [依赖/feature/API 兼容表和实际 SDK fixture](sdk-integration.md)。已验证自动 MRTR/discovery，并用反例记录必要宿主接点缺口，未宣称交互接入门槛通过。
- [ ] 2.0a 通过上游扩展/修复取得调用级 metadata、整轮预检、逐轮取消/网络期限、等待/检查及派发前 gate 接点；复跑 M-25 后才接 Grow 交互，不自写协议循环。
- [ ] 2.1 确定真实 Form/URL 服务和操作、入口/协议/transport 能力矩阵、schema 支持表、单项/累计字节及数量预算、期限和 localhost HTTP 策略。
- [ ] 2.2 升级官方 rmcp 并适配 reqwest/http/sse-stream/toolchain/lockfile，沿 mcp::rmcp 统一入口使用 SDK discovery/metadata；保留 no-provider TLS/ring 和最小 feature，核对 SSE/取消 workaround 的必要性及 transport identity。
- [x] 2.2a 完成 3.5.1 依赖升级及必要 API 适配；Rust 1.93.1 上 mcp/cli 编译、174 个原单测、3 个原 SSE fixture 和 9 个 SDK 探针通过，保留仍必要的 SSE/取消守卫。
- [ ] 2.2b 宿主接点门槛通过后，将生产 HTTP 入口接 SDK discovery/metadata，验证启动预算/回退及入口能力矩阵。
- [ ] 2.3 接通 SDK 自动 MRTR 与 ClientHandler，在 Grow 原 logical call 绑定轮次/key/owner、重复回答核对和取消/等待策略；隔离可能已发送请求的 recover-and-retry，不复制 SDK state/聚合/退避算法。
- [ ] 2.4 扩充现有 pending 管理的实例身份/整轮接纳，避免同 call 多项和旧 guard 互相删除；接入取消/断线清理、等待期间准入复核和最少 Timeline 事实，不新增 HumanIntent。
- [ ] 2.5 实现一致的 Form 类型/枚举校验、URL 和敏感数据边界；保留最终 typed result/附件处理，等待不持有共享锁。

## 3. TUI and verification — not implemented

- [ ] 3.1 实现表单与 URL consent/waiting，保留顶部 task 布局。
- [ ] 3.2 实现有界队列、统一焦点、URL waiting 让出输入、后台会话路由和草稿保全；重连不重置总期限。
- [ ] 3.3 运行 acceptance.md 的 M-01～M-25，分别记录依赖图/SDK 集成、wire、Shell、Pager/PTY 和真实服务结果。
- [ ] 3.4 更新开发者说明，执行变更所需 Rust/格式检查及 OpenSpec 校验；未通过前不归档、不宣称已实现。
