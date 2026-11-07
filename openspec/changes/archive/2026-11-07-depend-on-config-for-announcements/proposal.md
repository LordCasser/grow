# Depend on config for announcement state paths

## Why

backlog 的 announcements 依赖项已核实：本crate为取得 grow_home 依赖整个tools，唯一使用处是 announcements_state_path；tools::util::grow_home实际直接重导出config同一函数。

## What Changes

announcements直接依赖config并调用同一个grow_home，去掉仅为转发存在的tools依赖。函数、全局路径authority、持久化格式与行为都不变，所以skip_specs:true。Agent未消费public seam的用途核对是独立项目，不混入此依赖收缩。

## Impact

announcements manifest/唯一调用及lock对应本地package依赖边。无外部版本升级，不增加模块、配置或测试框架。
