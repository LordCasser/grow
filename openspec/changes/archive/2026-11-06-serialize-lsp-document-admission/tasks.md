## 1. Contract and reproduction

- [x] 1.1 核对Core原始失败、Documents/Client/router/store调用链和锁定async-lsp通知语义，确定单owner原子边界。
- [x] 1.2 以受控时序证明旧入队/commit窗口及诊断归属损失，保存原命令和失败证据。

## 2. Implementation and acceptance

- [x] 2.1 在现Documents同步状态边界完成成功入队/版本提交串行化，适配实际client，保留失败发送不推进和现诊断规则。
- [x] 2.2 完成受控red/green、open/change及发送失败、原push/pull和相邻版本/range回归；核对fmt、warning及源码边界。
- [x] 2.3 父核实际diff与命令证据，更新开发说明和验证记录，strict校验后归档。
