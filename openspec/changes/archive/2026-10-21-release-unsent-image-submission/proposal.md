## Why

Session `01a15c48-ab5b-7872-a263-15c28b8d8e69` 的图片提交在本地 checkpoint 失败后恢复了草稿，却仍保持 TurnSubmitting/current_prompt_id，watchdog 查询未发出的请求，重发被 turn_running 挡住。现有未保存草稿契约保留内容，但实现未释放发送占位。

## What Changes

- 本地 checkpoint 失败时，仅释放该请求仍占有的提交中状态和观察计时，保留图片、原身份与显式重试入口。
- 不自动发送，不关闭已经运行或由其他请求占有的前台。

## Capabilities

### Modified Capabilities
- `client-surfaces`：本地检查失败后的提交占位释放。

## Impact

仅 Pager checkpoint 结果处理、回归和开发说明。不改变图片本地持久化限制、Shell 接纳协议或模型图片能力恢复。
