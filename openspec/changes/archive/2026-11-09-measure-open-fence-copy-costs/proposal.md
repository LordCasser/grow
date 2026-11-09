## Why

MD-1 的开放 fence 增量高亮仍复制所有已格式化行；现有记录明确要求先量化 parse、copy 和渲染成本，再决定表示变化，不能把静态 O(N²) 累计复制直接当成正常 CLI 已失效。

## What Changes

增加 ignored、有限输入的测量测试，分别记录生产增量路径、独立 parse/style 机制、热缓存 clone、prefix 检查与真实末帧 Markdown render。输出与 batch 的 styled lines/source map 核对一致。

## Impact

测试和验证记录；不改生产 representation、依赖、公开 API 或性能门槛，`skip_specs: true`。采样不能证明所有终端或所有 Markdown 负载满足预算。
