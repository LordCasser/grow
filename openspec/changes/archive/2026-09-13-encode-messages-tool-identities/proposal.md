## Why
Messages 将中性工具 ID 的标点替换为下划线，a.b、a/b、a_b 会碰撞；Unicode 也会绕过 ASCII 检查。工具结果可能因关联身份损失而被 provider 拒绝或关联错误。

## What Changes
- 在每个 Messages 请求中为中性工具 ID 建立确定、无碰撞的 ASCII 编码，调用与结果复用映射。
- 保留已有短合法 ID，编码超长或非 ASCII/非法字符 ID；预留 native ID，完整保留原生续接块。
- 不修改 Timeline、原始请求、工具执行身份或其他 backend 的 ID。

## Capabilities
### New Capabilities
无。
### Modified Capabilities
- model-sampling: Messages wire 的中性工具关联身份。

## Impact
sampling-types Messages builder 及 portable/native 请求回归。

