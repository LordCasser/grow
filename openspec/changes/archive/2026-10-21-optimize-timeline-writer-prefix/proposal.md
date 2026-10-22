# Optimize Timeline writer prefix restoration

## Why
R5 的首次 writer append 仍逐事件调用事务性 Timeline::accept，累计克隆及析构 LifecycleFold。现代2048轮基准在 actor 启动阶段花费约6秒，CPU采样确认该前缀路径为热点。主 bulk fold 已消除同类累计复制，此处为现有契约的覆盖缺口。

## What Changes
保留有界逐行解码和原始字节 BLAKE3，收集拥有所有权的事件后调用一次 Timeline::from_events 做私有验证。不改变有效/无效历史的接纳边界、文件格式、前缀身份检查、append锁或文件及目录同步。仅修正内部算法，skip_specs，不虚构新契约。

## Out of scope
不改变 live accept 的失败原子性，不复用未经验证的前缀，不处理 R6 或其他债务。
