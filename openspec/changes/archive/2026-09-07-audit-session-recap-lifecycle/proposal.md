## Why
持续审计从 Hook 转入默认开启的 session recap，需要建立真实入口和并发/展示证据，不能用测试名称推断完整覆盖。

## What Changes
仅记录入口、已验证行为及待继续检查的边界，不修改行为契约。

## Impact
仅本 change 文档；skip_specs 为纯审计记录，不虚构 delta。
