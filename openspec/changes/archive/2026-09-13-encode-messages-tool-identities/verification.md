# 验证记录

日期：2026-09-13（Asia/Shanghai）。sampling-types 完整测试 278 项通过。

- messages_tool_ids_remain_distinct_and_paired_at_every_portable_cut：a.b / a/b / a_b / a-b、Unicode、4 KiB ID；逐个 portable 切点检查唯一性、ASCII/64-byte 输出预算、原始结果归属、原请求不变、重复请求确定性；另检查 Chat/Responses 原身份不变。
- messages_encoded_ids_avoid_later_native_and_neutral_identities：先取得编码候选，再将它作为后续真实合法/native ID，验证保留既有身份并消解碰撞；signed thinking 原样保留，native tool result 继续关联原生 ID。
- 既有 portable/native、六方向迁移和边界回归一并通过。

初次编译发现 Arc<str> 的 as_str 和 String/Arc 转换问题，已按实际类型修正；原始失败日志保留。普通合法 ID 路径加入早返回，避免每次请求都构造 reservation tree；最终补跑记录见整体验证。

只读交叉审阅另发现：独立 neutral/native exchange 复用同一个原始 ID 的歧义此前就可达，未由本次不同原始 ID 编码引入。已登记为请求投影的独立债务。未发真实 provider 请求；官方核对及本地长度预算的区别见 design.md。


完整构建、测试、依赖图及磁盘记录见 [同批整体验证](../2026-09-13-remove-runtime-dependencies-from-leaf-crates/verification.md)。
