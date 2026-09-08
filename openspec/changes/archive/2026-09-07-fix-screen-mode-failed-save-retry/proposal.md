## Why
屏幕模式未配置时选择 Fullscreen 需要显式保存，但失败回滚只保留 canonical fullscreen，导致 current_ui 被设置为 Some(fullscreen)。下一次选择相同值被当成无需保存，无法重试。

## What Changes
屏幕模式回滚保留未配置/原字符串状态，不用显示默认值替代缺失值。失败后重复选择会再次产生保存 effect。

## Capabilities
### Modified Capabilities
- client-surfaces: 屏幕模式保存失败的重试。

## Impact
Pager 屏幕模式 setter 与 rollback arm，使用既有 SettingValue::String 携带回滚原值，空字符串表示未配置。不改变成功保存或启动模式解析。
