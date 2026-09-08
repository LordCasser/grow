## Reproduction
旧 screen_mode_failed_initial_save_can_retry 失败：真实 dispatcher 接收保存失败后 current_ui.screen_mode 不再是 None。

## Validation
pager app::root::dispatch::tests::settings：113 passed。新回归从实际 setter effect 获取 rollback_value，回传 TaskResult::SettingPersistFailed 后检查 None，并再次选择 Fullscreen 确认新的 PersistSetting。移除无用导入后最终同组 113 passed；两次不相加。

## Limits
未写真实配置，不模拟磁盘故障或完整 ACP；验证 UI 状态与 effect 分发。多次并发保存归属单独记录 backlog。空原字符串与 None 按未配置等价处理。既有 linker __eh_frame 警告不影响结果。
