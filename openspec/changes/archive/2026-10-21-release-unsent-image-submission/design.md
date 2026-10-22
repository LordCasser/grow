## Context

图片经 queue drain 设置 TurnSubmitting 和 current_prompt_id，再经 process_effects 的本地 checkpoint gate。local_drafts 不保存图片，checkpoint 可返回 input intent was not captured；此时 send 尚未执行。现有错误分支只恢复草稿，导致后续 drain 永远被拦。

## Goals / Non-Goals

修复明确未派发的本地占位，复用现有 RAM-only 显式重试机制。不扩展草稿格式，不自动绕过 checkpoint，不把远端未知响应当成本地失败。

## Decisions

在 InputIntentCheckpointed 的 SendPrompt/SendPromptBlocks 错误分支，现有 session/binding/send identity 校验通过后，仅当 current_prompt_id 匹配且 state 为 TurnSubmitting 时恢复 Idle、清 current_prompt_id，并调用 mark_turn_finished 清理观察与计时。保留 in_flight_prompt 给既有草稿恢复与原身份重试。

不调用终态 finalizer：请求没有派发，不存在可宣布完成的服务端 turn。正常成功分支与远端未知接纳路径保持现状。

## Risks / Trade-offs

迟到 checkpoint 可能与其他前台或新编辑交错；用状态与身份双重门控防止关闭其他回合，沿用 requires_review 队列保留新编辑。图片仍需用户在未保存提示后显式重试，退出后仍不能恢复内存图片。
