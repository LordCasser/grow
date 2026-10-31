## Decision

fvec_from_value 是 vec_f32 和 vec_quantize_int8 的共同入口。由它将错误输出初始化为 NULL，比在每个调用方增加补丁更小，并保证无诊断文本时不会使用未定义地址；具体错误仍由原分支填入。SQLite 接受空错误文本，sqlite3_free(NULL) 安全。array_init 的失败没有可释放 vector，原 cleanup 边界不变。

测试在现有单 TU harness 中仅替换 vendored 源码的 sqlite3_malloc 调用，失败开关只覆盖被测转换，SQLite 引擎本身仍使用真实 allocator。用真实 sqlite3_value 验证失败返回 SQLITE_NOMEM 且原非空 sentinel 被清为 NULL；再经过真实 SQL scalar callback 验证两个入口失败、不崩溃且同一连接可以继续查询。有效及非法输入保留原返回语义。通过原 native suite 严格 C 编译及 ASan/UBSan，正式 RISC-V 构建再次核对原 warning。

禁止 suppress maybe-uninitialized、改 vendor 版本或移动已公开 tag；首轮确认 Release 不存在且 run 已取消后，才替换未发布 annotated tag。
