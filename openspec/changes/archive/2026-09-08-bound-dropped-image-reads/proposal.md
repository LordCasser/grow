## Why
实际拖拽/路径粘贴通过read_image_at_path执行无界fs::read，晚于此步骤的发送预算不能保护入口分配。
## What Changes
图片拖入读取限制50,000,000字节。文件句柄检查类型和大小，实际读取用limit+1；超限不产生图片chip，保留现有NonImage路径文本回退。
## Impact
pager-render图片拖入和测试，不改变URI查询/片段既有行为、发送链或旧builder。支持的正常符号链接路径继续可用。
