## 1. Repair

- [x] 1.1 核对actor/state/handle重复ID、JoinError及双collect Drop路径与Tokio任务身份契约。
- [x] 1.2 实施task匹配清理和每提交owned取消，保留public RequestId、协作结算与shutdown。
- [x] 1.3 公共Handle确定性回归旧任务迟到、collect Drop、panic隔离及正常cancel/shutdown。

## 2. Acceptance

- [x] 2.1 正确隔离工具链定向通过，独立review并验收最终diff与日志。
- [x] 2.2 同步规范/开发说明与backlog，完成整体CLI取消/重试里程碑调用链验收及归档前strict检查。
