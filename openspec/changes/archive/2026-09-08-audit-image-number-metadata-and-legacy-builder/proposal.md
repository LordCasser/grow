## Why
图片编号审计发现display_number_from_meta只有测试消费者，注释仍引用不存在的AttachedImages。进一步检查图片构建入口发现旧build_content_blocks链只被测试使用，不能把其中预算问题误报为当前发送路径缺陷。
## What Changes
记录生产与闲置链路证据，将删除候选写入临时清单等待用户确认。没有行为变化，skip_specs=true，不建立虚假delta。
## Impact
只更新审计文档和删除候选，不删除代码、不重构闲置组件、不改变图片编号或ACP元数据。
