## Why
SkillSlashRewrite 注释声称 RewriteToRun 和 Passthrough 会选择不同技能调用方式，但生产 resolve 不读取该参数。这会误导后续维护者。

## What Changes
核对调用链，修正过时注释，将闲置枚举和参数列入 R11 待确认删除。

## Scope
纯审计和注释修正，不改变行为或契约，skip_specs=true；保留类型、参数和调用点。
