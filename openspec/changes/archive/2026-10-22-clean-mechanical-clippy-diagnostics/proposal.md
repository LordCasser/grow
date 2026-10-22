# 等价 Clippy 整理

## Why

全 workspace Clippy 基线有 375 warning、4 error，需要先消除机械性诊断，以便单独审查资源生命周期和数据布局。

## What Changes

按固定 1.93.1 Clippy 整理等价表达式、借用、测试初始化与声明位置，复核机器可用建议并人工修正失效建议；去除已检查的 Option unwrap、无用 struct update/借用、简化等价控制流；修正扫描逐步暴露的测试 lint error。保留所有产品契约与错误断言，skip_specs。布局、进程、锁、路径策略和复杂类型诊断留给后续独立 change，不全局降级 lint。
