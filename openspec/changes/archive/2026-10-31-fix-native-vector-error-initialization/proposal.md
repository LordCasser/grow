## Why

2.5.5 首轮 RISC-V release 编译暴露 vec_f32/vec_quantize_int8 两处可能未初始化错误指针。fvec_from_value 的 JSON array_init 失败直接返回 SQLITE_NOMEM，没有写入错误输出；调用方仍将该输出交给 SQLite 并释放。

## What Changes

在既有 float vector 解析入口初始化错误输出，覆盖未生成诊断文本的失败路径；沿用 SQLite 错误和释放接口，不增加包装、重试或降级。补充确定性 allocation-failure 与正常/非法输入 SQL 回归，更新 vendoring 说明。发布保持 2.5.5，取消未公开流水线后将未发布 tag 绑定修复源并重新执行全部门禁。

## Capabilities

### Modified Capabilities

- memory-search：原生向量解析的错误输出所有权。

## Impact

vendored sqlite-vec 内部解析、已有 native_boundaries 测试；不改向量格式、有效查询结果或依赖版本。
