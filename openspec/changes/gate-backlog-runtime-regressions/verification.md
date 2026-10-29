# 验证记录

现有Core immutable-SHA门禁追加 diagnostics、pager-render、tty-utils、test-support；沿用失败阻断、线程数和期限。CLI构建后的既有交互阶段追加真实daemon普通Drop/模拟panic的精确ignored测试，不新建平行workflow。

本地对应全量包分别54、1022、29、71通过；真实daemon所有权1通过。Python脚本契约34 tests通过（1 skip），见 `/tmp/grow-2.5.2-script-tests-final.log`；workflow diff只增加包与精确测试，不改变release依赖。正式同SHA门禁执行结果由本轮发布记录补齐；未执行前不声称CI通过。
