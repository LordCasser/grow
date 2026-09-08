## Why
minimal 模式提前返回绕过 FPS 采样与绘制，/debug fps 显示开启却没有读数。需要把现有 HUD 接入 minimal 的真实绘制和 viewport 计算，而不是新增第二套统计或空闲计时循环。

## What Changes
对 minimal draw hook 计时，现有 HUD 采样；viewport 为顶部 HUD 预留两行，内容区域整体下移，空间不足隐藏 HUD。纠正旧 dev profiler 注释。

## Impact
minimal 渲染、FPS HUD 与窄 API seam；普通模式统计保持，禁用不增加采样。
