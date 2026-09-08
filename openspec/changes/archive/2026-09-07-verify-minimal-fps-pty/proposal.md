## Why
minimal FPS HUD 已通过 Buffer 和 minimal 单元测试，仍需验证真实 CLI 在 PTY 中的开关、统计显示和输入位置，避免仅凭几何辅助函数测试宣称终端集成可用。

## What Changes
增加隔离 PTY 回归，重建 CLI 并执行该用例，记录产物与结果。测试和构建验证不改契约，跳过 delta specs。

## Impact
复用既有 mock content server 和隔离 HOME/GROW_HOME，不调用用户模型或改用户配置。
