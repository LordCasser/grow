## 旧实现失败
duplicate_option_labels_rejected_before_send 超过 100ms 仍等待响应，0 passed / 1 failed；工具没有在发送前拒绝歧义输入。

## 最终验证
CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p tools --lib ask_user_question --quiet：66 passed、0 failed/ignored。
新增矩阵覆盖普通/ID 两种输出方式、不同 option ID/描述但 label 相同，均返回 Duplicate option label 且请求通道无消息。跨题重复 Yes/No 正常往返返回两题答案；既有问答格式与超时等测试保持。rustfmt 和 git diff --check 通过。

## 边界
按原样比较 label，不隐式 trim 或大小写折叠；不改变协议及外部客户端回答校验，未打开真实问答 UI。
