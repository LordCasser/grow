## Regression
旧实现的新增测试失败：name 和 args 中的双引号直接输出，产生 name="review"<&" 和 args="say "hello" ..."。修复后名称/参数/索引路径转义，正文 Raw <example> & **Markdown** 原样保留，重复引用输出一次。

## Existing expectation
build_skill_message_special_chars_in_fields 旧测试有意期待原样属性；保留原 description fixture，将预期改为实体转义。这是明确的包装行为变更，不是删除失败输入。预加载调用方使用同一包装函数。

## Results
- tools implementations::skills：104 passed，0 failed。
- agent prompt::skills::tests：96 passed，0 failed。
- 两次均 locked/offline、无增量、debug=0、2 jobs、RUST_MIN_STACK=16777216。
- 不声称执行 shell 或 CLI 端到端测试；本次未重新链接 CLI，未替换用户安装程序。
