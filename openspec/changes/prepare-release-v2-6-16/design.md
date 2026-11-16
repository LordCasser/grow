## Decision

沿用release.yml、annotated tag、clean-tree validate-release.sh、main原子推送与默认分支dispatch。新版本只有33个workspace继承包及notes变化，产品代码只新增一个Unix测试helper的cfg条件；已验收功能不重新设计。

## Review Plan

亲读cfg字段/调用mod的边界，复跑1.93.1的generation相关回归与静态检查，核对版本/lock无依赖漂移。复用v2.6.15本地13528项核心库和四项PTY的产品行为证据，正式2.6.16运行须重新绑定新SHA并完整通过Core、三OS原生协调、Windows storage、OpenSpec和十平台构建。实际验证remote十archive+SHA256SUMS、全量attestation门、macOS原生产物version/help，再归档任务。旧候选失败记录保留，不把其cancelled/failed job写成通过。
