## Why
近期技能路径管理、参数校验和后台重载修复已完成库测试，但工作区 CLI 仍为这些修改之前的产物。需重新链接并验证早期命令入口，避免交付旧二进制。

## What Changes
构建 main 工作区 CLI，执行版本与帮助烟测并记录磁盘。

## Capabilities
纯验证，无契约变化，skip_specs=true。

## Impact
target/debug/grow 更新，不替换用户安装路径。
