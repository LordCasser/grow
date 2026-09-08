## Why
剪贴板 snapshot 用较早的 changeCount 搭配稍后读取的 types，进程内锁不能阻止其他进程复制；此外 types=nil 被当作确认无图片。可能导致提示去重错误或粘贴门禁错误排除图片。

## What Changes
类型分类前后读取版本，变化或类型不可用时返回未知；保留稳定版本的图片/非图片结果。更新不准确的原子快照注释。

## Impact
client-support 元数据快照，现有提示和 attachment gate 共用；不改实际图片读取或新建重试循环。
