## ADDED Requirements

### Requirement: Reset reports durable permission-state outcome

ResetState SHALL 在 actor 中先撤销旧请求并清空内存权限域，随后通过独立串行写入保存最新根权限状态；调用成功完成 SHALL 表示权限文件写入已确认。写入失败 SHALL 作为明确错误返回，不得报告成功；当前进程的撤销仍保持生效。其他权限请求 SHALL 不因 Reset 的文件等待而串行等待。

#### Scenario: Reset succeeds
- **WHEN** 旧 remembered grant 存在且 Reset 的权限文件写入成功
- **THEN** Reset 成功返回，重载权限文件不包含旧 grant。

#### Scenario: Reset persistence fails
- **WHEN** Reset 已撤销在途旧请求，但权限文件无法写入
- **THEN** Reset 返回错误，当前进程不恢复旧 grant，也不把旧请求的迟到允许采纳为新授权。
