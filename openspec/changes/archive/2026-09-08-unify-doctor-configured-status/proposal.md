## Why
CLI在报告返回前检查当前shell目标中的托管SSH alias，TUI只统一tmux目标。相同已配置状态下，/doctor仍重复建议SSH修复。
## What Changes
将报告的持久化配置判定和tmux目标提示放入共享收尾函数，CLI/TUI均调用；本地SSH判定使用已有shell目标解析，远程不以远端alias隐藏本地建议。
## Impact
诊断报告收尾、CLI/TUI调用点和回归；不执行shell，不改变alias写入或实时路由探测。
