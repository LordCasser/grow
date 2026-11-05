## Why

WS-06a真实Handle实验确认watcher未交付的外部编辑会被reject从旧cache生成的整文件覆盖。actor串行化自己已接收消息，不拥有外部磁盘writer；展示cache不能充当磁盘当前版本。

## What Changes

hunk reject在实际动作前核对文件存在性和完整内容是否仍等于本次展示的current snapshot；不一致明确返回stale冲突并保全磁盘/未提交投影。所有single/file/turn/all入口收敛于同一reject owner。先以commit-hunk-projections-after-success提供失败提交边界，再落本检查。

## Capabilities

### Modified Capabilities

- client-surfaces：hunk reject的stale磁盘冲突与不覆盖语义。

## Impact

hunk-tracker action/types/tests和必要消费者错误映射；复用现错误传播到Shell，不新增三方合并/全局文件锁/并发编辑框架。只保证动作读取时已不同的版本被检测，不虚构非合作外部writer的跨syscall原子compare-and-swap承诺。
