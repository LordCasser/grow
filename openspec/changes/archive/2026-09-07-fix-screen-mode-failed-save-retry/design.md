## Evidence
set_screen_mode 比较 raw 值与 canonical 值以允许初次确认 Fullscreen，但 rollback_value 只保存 canonical(prev)。apply_setting_rollback 又设置 Some(canonical)，从而丢失未配置状态。

## Decision
保存 effect 的 value 仍为 canonical Enum；rollback_value 改为原字符串（缺失用空字符串）。rollback 直接恢复 Option，不再次 canonicalize，保留非空未识别原值也能再次选择。空原值与缺失保持等价，不引入新枚举变体。

## Limits
不处理多次异步保存结果的先后归属；该问题需要独立核对请求身份，不混入本次缺失值回滚。
