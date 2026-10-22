# 内部消息承载与类型整理

## Why

机械性清理后的完整扫描仍有 20 条 large_enum_variant、18 条 type_complexity、8 条 result_large_err。大 payload 使小事件和 mailbox 项也承受相同内存尺寸，嵌套类型与错误传递需要逐项核对。

## What Changes

使用标准 Box 间接承载尺寸明显悬殊的内部 payload，必要时压缩错误承载；复用 futures 标准别名或命名现有复合类型，不引入运行时框架。保持序列化、schema、摘要、Timeline seq/身份、ACK、历史顺序及错误分类不变。只改 Rust 内部布局/表达，不改产品或持久化契约，skip_specs；资源与路径诊断独立处理。
