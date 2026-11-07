# Proposal: settle-late-question-replies (调查 only)

## Why

用户报告：一次 `ask_user_question` 长时间无人回应，所在任务已经以其他方式完成；随后用户补答了 ask，主会话疑似卡住。本 change 只做**只读源码调查 + 可证伪最小 fixture 设计**，不改行为、不改产品代码。非阻塞 ask 的产品需求由 `feature-ask-blocking-choice` 单独拥有，本调查仅为它提供 terminal question identity 的事实基础。

## What Changes

- 梳理 ask_user_question 的真实通知链：tool → shell user_question_worker → ACP ext_method `grow/ask_user_question` → pager QuestionViewState → pager submit → oneshot 回传 → worker → tool result_tx → tool loop。
- 分辨blocking当前请求、tool已超时/取消以及迟答的实际可达性；分别核UI提交、两层oneshot和持久resolution，不能从silent send失败推导主会话卡住。
- 记录owner/state/代际ID、竞争解释与反证；实际运行worker最小判别和回归，保留未复现的用户报告。
- 父核结论与证据限制见[验证与裁决](verification.md)。本change不改产品行为，`skip_specs: true`；推荐继续/迟答新输入由独立`offer-advisory-user-questions`行为change实施。

## Impact

证据文件：`design.md`（事实与链路）、`tasks.md`（fixture 与验证计划）。不写主 spec、不写 docs、不登记 backlog（债务由父会话决定去向）。
