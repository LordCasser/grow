## Why
近期修复已通过相关库测试，需要确认 main 工作区可以链接实际 CLI。

## What Changes
执行低磁盘占用的 CLI 构建，并验证版本和帮助入口；仅记录验证结果。

## Capabilities
不改变产品契约，skip_specs=true。

## Impact
生成 target/debug/grow，不替换用户已安装的二进制。
