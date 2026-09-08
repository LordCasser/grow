## Why
配置写回审计需要确认显式 save_config 是否仍有实际调用，以免把未使用的入口当作现有产品风险或误删仍在运行的底层持久化代码。

## What Changes
记录全仓调用证据，将无仓内调用的公开 save_config 及专用 save_config_locked 包装列为删除候选 R7。修正 settings_writes 的过时调用链注释，不删除代码。

## Capabilities
无契约变化，skip_specs=true；这是审计和注释修正。

## Impact
审计记录、临时删除清单、一处注释。公开 Rust API 仍可能有仓外消费者，删除前需用户确认并复核。
