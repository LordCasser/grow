## Reproduction
旧实现混合加载回归失败：正文只有 available，skills_referenced 却包含 available/missing/gone 三项。missing 文件不存在，gone 不在当前目录列表。

## Validation
低磁盘配置 `cargo test --locked --offline -p shell --lib session::slash_commands::tests --quiet`：97 passed。新混合场景与原有正文/参数替换、全失败 None 等测试通过。存在既有 macOS linker unwind 警告。

## Scope
生产 turn admission 与 interjection 共用被修复函数；未执行真实多 turn 会话 E2E。原始用户引用文本与上游成功诊断统计未改变。CLI 未重新链接，target 9.8 GiB，可用约 68 GiB。
