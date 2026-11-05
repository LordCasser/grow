# Retire sampling state with a cancelled Turn

## Why

Sampler任务身份修复的真实CLI里程碑发现另一个Shell终态owner缺口：原生ACP取消一个已开始的流后，新用户Turn的第一次HTTP请求继承旧恢复计数。固定新构建的两次隔离实测均有五次前台HTTP请求，但第三个Turn的503/成功分别登记为新RequestId的attempt 2/3，而非1/2。当前max_retries=3时能成功，较小上限可能因此提前耗尽。测试没有放宽断言。

## What Changes

- SessionActor取消owner在真正提交Turn terminal后退役该Turn的瞬态sampling continuation，连同同一epilogue负责的turn continuation。
- 新用户Turn从独立logical sampling预算开始；旧已准入attempt的用量/evidence继续由其原owner结算。
- 保留挂起/恢复同一未接纳步骤的预算、绝对期限及重放门槛，保留普通正常终态原行为。

## Impact

独立于已验收的Sampler任务身份修复，只改Shell取消终态及同一行为必要测试。使用已有真实CLI场景保持[1,2,1]与五个attempt结算身份断言；过滤具有真实前台RequestId的evidence，session-title Sideband保留自己的独立owner。父拥有此change、开发说明、backlog与归档。没有新增schema、持久状态、公共API、依赖或通用清理框架。
