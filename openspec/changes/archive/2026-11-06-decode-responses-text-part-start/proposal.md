# Decode initial Responses text parts without annotations

## Why

用户指定会话01a1ae61-3441-7c22-af81-f5101281e92e两次HTTP200流在 `response.content_part.added` 的空 `output_text` part 缺少 `annotations`，锁定SDK在进入L2前报 `missing field annotations`。同会话成功sideband在added/done/terminal均带空数组。失败artifact没有记录terminal，不能推断完成项的缺省规则。

用户明确要求调查修复归入下一版本，不中断正在发布的2.6.1。本change不修改v2.6.1/deaca4899，不重启或重放原会话，在隔离工作区验证后保留为后续版本修复。

## What Changes

只在已知 `response.content_part.added`、`part.type=output_text`、`part.text=""` 且 `part.annotations` 键不存在时，为SDK解码副本补空数组。保留已有annotations、原始evidence和SDK对其他字段/重复字段的拒收；done、terminal、非空初始文本及其他事件不采用该例外。L2及最终响应验证继续权威。

## Impact

所有者为Sampler client已有typed-decode边界。无SDK默认值、依赖、公共类型、自动重试或预算变化。开发说明随独立验收更新；2.6.1发行说明不追加该未发布修复。不新建通用JSON兼容框架。
