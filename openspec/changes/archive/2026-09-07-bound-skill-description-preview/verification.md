## Reproduction
将原完整读取逻辑提取为生产预览 helper 后，读取预算测试旧实现失败：预览长 61440 而非 6144 bytes。

## Final validation
低磁盘配置 tools `implementations::skills` 99 passed，agent `prompt::skills::tests` 96 passed。新回归直接检查底层 Cursor position 为 6145，截断中文字符保留前缀、内部非法字节与 EOF 残缺 UTF-8 返回错误。既有描述提取与发现回归通过。

## Limits
描述仅依据有界前缀，长前导空白等内容可能不再产生此前全文扫描得到的描述。frontmatter 读取单独计费；显式正文加载没有修改，不宣称它已具备容量上限。CLI 尚未重新链接。磁盘可用约 69 GiB，target 9.8 GiB。
