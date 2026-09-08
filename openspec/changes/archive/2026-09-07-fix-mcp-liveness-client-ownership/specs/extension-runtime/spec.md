# Delta

## ADDED Requirements

### Requirement: Liveness watchers do not own idle clients
MCP liveness watcher SHALL 在检查间隔只持有客户端弱引用，不因长期监测延长客户端生命；发现客户端已释放时 SHALL 清理自身槽位并静默退出。

#### Scenario: 最后外部引用释放
- **WHEN** 检查间隔或首次 tick 前最后一个外部客户端强引用释放
- **THEN** watcher 不阻止客户端销毁，后续 tick 清理自身槽位且不产生 TransportClosed。
