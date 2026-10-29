# 验证记录

现有Core immutable-SHA门禁追加 diagnostics、pager-render、tty-utils、test-support；沿用失败阻断、线程数和期限。CLI构建后的既有交互阶段追加真实daemon普通Drop/模拟panic的精确ignored测试，不新建平行workflow。

本地对应全量包分别54、1022、29、71通过；真实daemon所有权1通过。Python脚本契约34 tests通过（1 skip），见 `/tmp/grow-2.5.2-script-tests-final.log`；workflow diff只增加包与精确测试，不改变release依赖。正式同SHA门禁执行结果由本轮发布记录补齐；未执行前不声称CI通过。

最终2.5.4源 `069f712894caf83352ace5b0e71276f07f01f9e1` 的push Core `40836525699`与正式release Core均success。正式 [run40841704026](https://github.com/LordCasser/grow/actions/runs/40841704026) 20个job全success：新增四个库包、真实daemon Drop/panic精确测试及全部28项PTY均经同SHA门禁执行。早期新门禁确实拦截了诊断测试时序假设和旧活动组PTY契约，分别独立修复并保留失败候选tag，未绕过。发布资产已完成独立checksum/latest/native version/attestation核对；最终发布记录保存完整公开metadata与job清单。
