## Why

Grow 2.6.2 的发布 Core 在关联会话删除错误应包含 child ID 的断言偶发失败，而同一提交的独立 Core 通过。检查发现 session writer lease 只关闭文件：Unix 子进程在 fork 到 exec 之间继承同一 open file description，会让已退役 writer 的锁继续存在，阻止合法替代 writer 和删除资格检查。

原日志没有输出实际错误，不能将该历史失败断言为已证明的同一原因；本变更通过确定性继承描述符回归独立证明并修复该锁生命周期缺陷，同时补全原断言的诊断。

## What Changes

- 让已有共享 writer lease 的最后一个 owner 或显式释放路径主动解锁，再关闭文件。
- 保留真实 adapter clone/持久化 owner 持锁、并发 writer 拒绝、关联会话删除保护和 TTL 闭链资格。
- 增加确定性 Unix 子进程继承描述符回归；保留并增强原清理矩阵断言。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `session-timeline`: 明确完整 writer 退役释放 OS 锁，不由无 writer 权限的继承描述符延长；真实共享 owner 仍阻止替代。

## Impact

限定 Shell JSONL writer lease 持有类型、存储测试和对应开发说明。不改变持久格式、公开接口、删除资格或流程时限。不增加重试、等待、测试串行化，不扩大到其他文件锁。
