## ADDED Requirements

### Requirement: Minimal mode renders the enabled FPS HUD
minimal 模式 SHALL 将已启用 FPS HUD 接入真实 draw hook 耗时采样，并在空间足够时展示现有统计。

#### Scenario: Enabled minimal frame
- **WHEN** HUD 开启且 live viewport 至少可容纳 HUD 两行和正文三行
- **THEN** 顶部显示 HUD，正文及光标布局使用剩余独立区域，不被覆盖。

#### Scenario: Disabled or tiny viewport
- **WHEN** HUD 关闭或空间不足
- **THEN** 不占用 HUD 行；关闭时不记录样本，输入区域优先保留。

#### Scenario: On-demand measurement
- **WHEN** minimal 已有绘制发生
- **THEN** 记录该绘制路径耗时，不增加空闲刷新循环，也不将后台 PTY 完成或屏幕刷新率宣称为采样结果。
