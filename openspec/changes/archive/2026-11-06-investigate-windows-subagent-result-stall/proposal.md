# Investigate Windows subagent result stall

## Why

用户报告Windows上subagent已打印Result，却没有结束或向父任务递交。本轮需区分模型文本可见、Turn终结、执行返回、canonical结果、父级Ended和完成回执；不能仅凭UI文字认定哪一层成功，也不能预设为Windows管道或协议切换问题。

## What Changes

用户已明确无法取得现场，改为纯代码分析。本轮核当前源码的结果/结算/交付所有者、Windows文件实现、现有契约和测试边界，输出确定顺序错误、期限覆盖缺口与条件性停滞路径，并将互相独立的后续处理登记backlog。交付以该分析和文档校验为终点，不再等待Windows trace，不实施修复。

skip_specs:true仅适用于当前调查文档，无生产行为或契约变化。若证据支持实施，先补对应delta/design/tasks再交单一owner的coder；不把旧的Released(None)修复、SessionEndHook债务或当前模型请求错误当作本现场已证根因。不重启/杀Windows进程、不重放子任务、不手改Timeline或伪造terminal。
