## 审计边界

从 `01a16811-f823-7a52-8b64-9a3ab105151a` 的本地 Timeline 核对截图，读取截止到相应 domain seq 的事实，不修改 session，不调用 provider，不执行历史工具。仓库已有的 rewind/route rebase 修复和其他未提交改动保留。

事实证据使用事件序号、时间、数量和估算值，不复制用户业务正文。区分三种证据：历史日志直接事实、按当前生产算法的算术重建、未运行 Rust fixture 的源码控制流审计。不会将本地 bytes/4 估值称为 provider 的精确 token 数。

审计入口：ChatState mutation/request projection；Shell manual/synchronous/background/promoted compaction、model-switch/pre-sampling/preflight/provider-overflow；Pager 的 activity 与 live feedback。检查以同一计数坐标、提交与请求准入的分离、Started 必须有终态为不变量，不新建运行时管理层。

具体证据、差异、边界矩阵和建议见 [analysis.md](analysis.md)。
