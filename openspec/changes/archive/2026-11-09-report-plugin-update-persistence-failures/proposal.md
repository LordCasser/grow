# Report plugin update persistence failures

## Why

AC-07 已冻结让 ACP 复用共享 update application，但共享函数仍用容错 load 和 save_registry_or_warn；普通 Git checkout 已推进、登记保存失败时仍返回 Updated。AC-08 本子项补齐同一 owner 的 durable 结果边界。

## What Changes

普通 update 在任何 Git/marketplace 副作用前 fallible load，普通 Git 更新登记保存失败时向 CLI/ACP 传播明确错误及已发生的 checkout 更新事实。复用 marketplace 现事务保存/rollback语义，不改变其协议，也不把已成功的其它 repo 伪称回滚。无 ordinary registry 变化时不增加无意义保存。

## Impact

共享 update application及既有ACP/CLI错误消费者、该行为的故障注入测试；不改变选择器、require-SHA、同tip no-op或另两项安装/卸载语义。
