## 1. Freeze milestone

- [x] 1.1 记录用户停止点、接受成果和未修backlog，结束在途必要验收且不新增审查。
- [x] 1.2 完成索引、Sampler和取消终态等已接受行为的场景/证据核对、文档与OpenSpec归档。

## 2. Prepare local version candidate

- [x] 2.1 准备2.6.1发行说明，更新workspace继承版本和lock本地包版本，保护独立/third-party版本及用户dirty。
- [x] 2.2 通过版本边界、cargo fmt及既有release脚本测试，清理生产/workspace-all-targets实际编译warning，完成最终默认CLI check/build并记录真实产物、源hash和未执行平台门禁。
- [ ] 2.3 strict/archive检查并形成具体发布候选，创建匹配提交/tag并核对既有发布门禁。

## 3. Publish the authorized version

- [ ] 3.1 push候选提交与annotated v2.6.1 tag，按精确SHA运行现官方门禁和全平台release workflow。
- [ ] 3.2 核对官方Release、十个平台资产、SHA256SUMS与attestation，记录实际发布结果和剩余平台限制。
