# Measure ACP gateway backlog

## Why

ACP-1先要求受控慢handler/producer实验，当前仅有unbounded channel与spawn-per-message静态证据。实验区分queue积压和active handler，不能把某一有限负载量宣称正常CLI失败阈值。

## What Changes

增加acp-transport的ignored受控测量与验证记录，不改生产dispatch、容量或错误语义，因此skip_specs:true。测量结论交父确定必要预算政策，行为实现另立delta。

## Impact

仅acp-transport gateway测试及本目录，不改public API/依赖/CLI。详细前提见../complete-nondeferred-backlog/acp-capacity-preparation.md。
