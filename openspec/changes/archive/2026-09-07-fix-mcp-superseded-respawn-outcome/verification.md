# Verification

- 旧实现 superseded_final_respawn 回归：1 失败，末次配置失效仍出现4条状态（预期仅前2次真实失败）。
- 最终 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib mcp_ --quiet`：126 通过、0 失败、0 忽略；仅已有 __eh_frame 链接警告。
- 新回归覆盖首次/末次 Superseded：无该次失败和耗尽推送、无工具注销、立即停止；已有普通故障、三次耗尽、取消、dispatcher/e2e测试通过。
- 生产 actor 的配置缺失、启动后的generation变化、握手后的generation/内容变化均映射 Superseded；启动/握手错误也先复核配置再分类，不解析字符串。新客户端照旧在失效返回时丢弃。
- 新回归使用 MockActions 验证循环行为，未启动真实子进程并重写项目配置；生产generation检查与结果绑定由源码核对和Shell编译验证。
- 同名工具前缀碰撞经入口代码排除：parse_mcp_qualified_name 要求唯一不重叠分隔符，into_registration 拒绝含歧义分隔符的server/tool。
- 成功提交后的监听器安装交错仍需后续核对，本次不宣称完整配置变更生命周期无竞态。
