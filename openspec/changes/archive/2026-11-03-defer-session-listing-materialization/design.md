现有Summary已持有DateTime<Utc>，SessionListing字符串仅供传输。旧实现stable sort_by_cached_key(Reverse(parse(last_active_or_updated)), session_id)，去重空cwd后truncate。新实现对query后的Summary稳定排序，按同规则去重并take(limit)，只为保留项调用summary_to_session。worktree标签仍在构建时沿现有lookup，配置/存储身份不改。

验证：旧算法reference与新入口全字段serde值差分，含纳秒/tie、缺last-active、0 limit、同cwd多空会话、非空重复cwd、query Unicode/ID/title与稳定完全重复key；已有API caller回归。单进程warm pure函数fixture在128/512/2048/9864 Summary、limit30，计时外准备输入，前后同样本；实际磁盘/DB benchmark另有现有9,864summary基准，此处不混淆测量阶段。
