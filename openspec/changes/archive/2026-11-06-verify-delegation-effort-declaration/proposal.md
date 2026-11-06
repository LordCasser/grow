# Verify delegation effort declaration across running versions

## Why

用户报告会话01a1a9c0-03b9-76d0-b513-923efdcfd80a声称委派工具没有effort参数。原请求证据确认当时的spawn_subagent确无该字段，运行日志为Grow2.6.0。2.6.1已实现并发布独立effort与resume覆盖，因此需要核实实际发布包声明并说明运行版本边界，不能为旧进程再加重复功能。

## What Changes

只记录原请求schema/运行版本/固定2.6.1源码及隔离发布包验证，补充用户指南的进程版本排查。skip_specs:true：不改变工具schema、行为、持久化、权限或恢复契约，不虚构行为delta。用户进程/安装/配置不改，不调用原provider或重启原session。

## Impact

本change仅文档和验收记录，属于2.6.1之后的维护。已公开2.6.1源码/tag/资产不变；若实际发布包声明出现差异，再按事实建立独立行为修复，不能预先假定新缺陷。
