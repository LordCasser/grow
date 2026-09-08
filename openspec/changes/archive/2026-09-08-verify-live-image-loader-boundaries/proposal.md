## Why
实际发送已使用append_prompt_images，但现有测试主要覆盖数量、缺失文件和大小预算，缺少内存/文件来源一致性及Unix特殊文件拒绝证据。
## What Changes
只为现有实现补回归：相同编码数据的两种来源保持一致，内存数据可用时不依赖磁盘副本；符号链接、目录和FIFO必须快速拒绝。skip_specs，因为不改变现有实现或契约。
## Impact
pager root effects测试和验证记录；不访问真实用户文件、不修改发送实现。
