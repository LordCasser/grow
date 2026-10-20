## Why
用户要求补齐 show_model_provider 造成的四项 Settings integration 清单遗漏，确认回归通过后提交、推送并发布；另要求举一反三分析类似配置入口/覆盖缺口。

## What Changes
补齐 show_model_provider 的覆盖矩阵、Bool 类型成员、默认值与 typed setter 清单，并添加键盘/鼠标交互覆盖。将完整 settings_e2e 接入既有 Core regression 门禁，避免该套测试长期不随发布运行。核对现有设置注册与保存/重置/回滚、UI 配置和独立配置入口，记录类似遗漏及预期入口。

## Boundary
仅测试维护和只读审计，skip_specs，不改变现有配置或产品行为。已授权的子 Agent 权限菜单改动由独立归档负责。确认的其他行为缺口单独建立 change 后处理，审计不把所有 TOML 字段直接映射成菜单。
