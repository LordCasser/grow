## Why

v2.6.15官方Windows的storage与native coordination门禁均在编译shell lib tests时报告E0609：IndexedHistory.timeline_generation访问仅Unix存在的tools字段。该helper是本轮新增测试观测接口，真实索引与其调用测试均只适用Unix，macOS全库未覆盖非Unix cfg。

## What Changes

把helper的测试编译条件与既有Unix索引所有者一致，保持非Unix运行时内存历史路径，不加假值、stub或兼容fallback。

## Scope

skip_specs：仅修测试辅助项的平台编译资格，不改变生产行为、接口、权限或持久化契约。按真实Windows门禁重新验证，不用macOS冒充Windows通过。
