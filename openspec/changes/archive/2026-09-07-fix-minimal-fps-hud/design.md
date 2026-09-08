## Design
AppView::draw 在 minimal hook 前后按 enabled 计时并 record。minimal API 暴露 HUD overlay 和行数查询。FpsOverlay 提供共享行数策略和顶部绘制/剩余内容矩形，至少为正文保留三行。compute_target wrapper 从终端可用空间扣除 HUD 再调用原内容高度计算；draw_live 先清区域，再绘制 HUD 并将返回的内容矩形用于全部分支。已有 resize/restore 机制处理关闭后的收缩，不新增调度器。

## Validation
HUD Buffer 测试覆盖可见统计、非零坐标、内容边界、极小高度和禁用；minimal viewport 测试/现有 live 测试覆盖组合布局。采样行为通过真实 hook 路径代码核对，记录不等于 PTY 完成。
