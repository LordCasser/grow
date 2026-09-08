## Why
多技能展开时正文只保留加载成功项，引用索引却包含全部请求。部分失败后模型会看到不存在正文的已加载引用，需要让索引与实际成功集合一致。

## What Changes
仅在正文加载并生成块成功时加入技能引用索引。

## Capabilities
### Modified Capabilities
- configuration-rules: 展开提示的引用一致性。

## Impact
shell 共享技能展开函数，影响 turn admission 与 interjection。
