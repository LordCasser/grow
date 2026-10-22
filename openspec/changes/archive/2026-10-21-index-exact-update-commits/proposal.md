## Why

Exact projection/ACP commit 在追加前后重新读取全部 updates；长会话的持续提交重复解码旧历史。用户要求实施算法审计 S1，并比较前后基准。

## What Changes

- 在现有存储 writer owner 内按已验证文件身份和 stamp 维护 updates identity 索引；普通追加同步维护，外部变化使索引重建。
- 只读取与目标身份匹配的记录核对完整 payload，保留重复冲突和原持久化同步屏障。
- 增加独立基准及替换、混排、损坏尾、同步失败重试回归。

## Capabilities

不改外部或持久化契约，skip_specs 为 true。仍以完整 payload、file/directory sync 为成功依据，索引仅是同 owner 可重建的加速状态。

## Impact

限定 shell session JSONL 存储及测试；不改格式、调用接口或 actor 放行顺序。
