## Source and version

基线 main bdfbc11e；正式版本只提升 33 个继承 workspace 产品版本的本地包，24 个独立版本保留，外部依赖及 MCP 锁定图不升级。release notes 对照 v2.4.0..main 的已归档行为，涵盖 Windows 导出、Skill、视觉/通信、Plan、采样恢复、挂起性能与失败路径；MCP 多轮交互保持暂缓。

## Git and publication

只删除证明为 main 祖先且无独有提交的旧分支；缺失 worktree 登记先核对，存在的非托管工作树可在原 HEAD detach 保留文件，托管临时 worktree 在不再需要时归档。MCP 分支明确排除；未合入独有提交不因年代旧而删除。

正式候选清洁提交 main，建立 annotated v2.4.1 tag 并运行完整 validate-release.sh，再推送 main/tag，由默认分支 dispatch 官方 release.yml，全部平台组及 publish=true。等待同 SHA 回归门禁和十平台构建完成，通过 public 状态、资产集合、checksum、归档入口与原生可运行产物核对发布结果。记录实际失败与限制，不把本地 macOS 编译当作跨平台验证。
