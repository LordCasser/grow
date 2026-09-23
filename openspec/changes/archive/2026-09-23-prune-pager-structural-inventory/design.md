## Decision

Pager backlog 只保留能回答“什么输入或状态会出问题、影响是什么、如何验证”的条目：

- 保留 process-global 状态、后台 worker/队列、缓存生命周期、持久化版本、坐标/宽度、身份映射、授权 epoch、输入边界和用户可见诊断等具体不变量。
- 保留明确的默认覆盖缺口（被 `#[ignore]` 的场景、绕过交互路径的 fixture、Markdown 属性检查和 bracketed-paste 边界），但删除只批评断言风格、等待时长、模块大小或 fixture 组织的条目。
- 删除 327–445 的全部“待拆分”标题/段落；它们只登记文件职责和计划拆分，没有独立行为证据。447–680 的行逐条重判，结构性建议删除，具体问题合并为短句并保留触发条件。

不根据文件名、归档 change 名称或静态条目数量推断问题已经修复。已有 Pager archive 只证明其各自行为范围；本次不修改或重写这些 archive。

## Scope boundary

本 change 不加入新的 spec delta，不运行 Cargo 或测试，不改变任何 Rust/Python 实现。验证只检查 backlog 的删除/保留边界、显式链接、Markdown 格式和 OpenSpec 文档结构。
