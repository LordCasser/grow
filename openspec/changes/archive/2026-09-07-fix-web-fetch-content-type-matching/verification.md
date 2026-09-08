# Verification

- 旧实现运行 content_type_classification_ignores 回归：1 失败，首个 TEXT/HTML 断言即失败。参数误匹配由 contains 实现直接确认；最终矩阵覆盖该边界。
- 修复后 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p tools --lib implementations::grow_build::web_fetch:: --quiet`：131 通过、0 失败、0 忽略。
- 测试实际执行 process_text_content 验证大写 HTML/XHTML 转 Markdown、纯文本参数中含 text/html 时保留原文；PDF 精确匹配使用分支谓词测试，未新增真实 PDF 下载测试。现有 PDF/HTML 和其他媒体类型测试一并通过。
- 不访问网络或用户会话文件。只修改两个分类函数，其他 MIME、媒体魔数和保存策略未变。
