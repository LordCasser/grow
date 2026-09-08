## Confirmed evidence
- skills::reload_skills 在配置加载后 timeout(5s, list_skills_with_plugins)，Elapsed 映射 vec![]。
- agent::list_skills_with_plugins 仅 await list_skills_with_roots；后者没有 await，执行同步 git、目录枚举、技能解析。插件和配置技能扫描也同步执行。
- tools discovery::walk_for_skill_md 使用 std::fs::read_dir，解析使用 File::open/read_to_string。
- Cargo.lock 的 Tokio 1.53.1 Timeout::poll 先 poll 内部 future，Ready 即返回 Ok，只有 Pending 才 poll deadline；不能打断同步扫描。
- PluginRegistry 可 Clone，允许移交只读快照。

## Implementation direction
将扩展重载的配置加载与发现放入 blocking worker，保留 owned cwd/registry 快照。使用扩展范围共享并发许可，许可由 worker 持有至真实退出；超时只结束等待，不声称中断 OS 文件调用。截止时间覆盖排队和执行，超时后的后续请求不得无限创建 worker。

返回显式错误，不以空列表冒充成功。add/remove/reset 已经完成的配置写入仍然保留，错误必须如实表述重载失败，避免误报保存失败。合法空目录仍返回成功空列表。

## Boundaries
其他 session/inspect/workflow 发现调用者单独审计；不混入此入口修复。不要只用 spawn_blocking 后丢弃 JoinHandle 来假称取消完成。
