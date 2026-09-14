# 验证记录

日期：2026-09-13（Asia/Shanghai）。最终整体记录见同批依赖重构的 verification.md 与 validation/ 日志。

Shell 完整库测试 3,795 通过、3 ignored。与本 change 直接对应的场景全部通过：
- coordination_observer_preserves_live_sideband_and_new_writer_recovers_once：live writer + lagging title 下 full/light 观察成功，Summary/sideband 字节不变，观察者两个 writer cache 均为空；拒绝第二 writer，原 writer 退出后修复 title 并只追加一次 sideband terminal。
- model_change_observation_is_read_only_and_replacement_writer_repairs_summary：模型/effort 滞后时同样验证内存值、原文件字节与 lease；新 writer 接管后落盘。
- load_repairs_a_lagging_title_projection_from_timeline：保留原 writer repair 场景，明确调用 writer load。
- title 冲突及 malformed_model_change：full/light 与 writer 都拒绝。

源码复核：load_light_data 的 restore_for_write 同时控制 sideband recovery 与 title/model 持久修复；full load 固定只读。返回的 canonical 内存 Summary 不回写 observation handle。

平台限制：本机 macOS 已运行；Windows 沿既有 shared-read 能力路径，未在本机执行 Windows runner。没有宣称跨文件原子快照。


完整构建、测试、依赖图及磁盘记录见 [同批整体验证](../2026-09-13-remove-runtime-dependencies-from-leaf-crates/verification.md)。
