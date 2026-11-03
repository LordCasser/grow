## 独立对抗性审查
reviewer `/root/rules_recovery_review`，独立静态审查，未运行Cargo。

1. 初审发现正向fixture在每次restore前补造预期出生集合，掩盖生产记录错误。改为suspend先核对真实持久值，保存原始Timeline；负向阶段仅修改目标字段，正向还原原字节。
2. 发现pool.clients快照与actor live import并非同一采样。明确logical Spawn admission冻结grant，新增现有scope的Bindings形式约束server/client ID，从同一静态样本持久名字、建立DelegableCeiling。actor重复freeze幂等，实际可用性仍与live upstream取交；普通pool保持live rebind。
3. 复审发现先live检查父ceiling再freeze仍有跨线程同名ID替换窗口。修正为先freeze，再仅对静态client_bindings求父ceiling交集；之后不再live决定出生grant。

最终复核确认以上问题均修复，未发现剩余可行动正确性问题；普通tool当前definition重核对、Workflow frozen源和root配置无额外扩展。
