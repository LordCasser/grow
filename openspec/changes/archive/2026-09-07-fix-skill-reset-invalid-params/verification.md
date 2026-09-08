## Evidence and intermediate failure
源码确认 reset/config 的 unwrap_or 将反序列化错误变成默认请求，reset 后续调用 update_config。改为 parse_params 后，输入矩阵仍失败：derive 接受 [] 为 CwdParams {cwd:None}。因此增加对象限定反序列化，而非删去失败场景。

## Final result
低磁盘配置执行 `cargo test --locked --offline -p shell --lib extensions::skills::tests --quiet -- --skip test_resolve_tilde_path`：17 passed。参数矩阵拒绝 null、空数组、字符串元素数组、数字/布尔 cwd，接受空对象、cwd:null、字符串 cwd；验证 invalid_params 错误码。旧 HOME 修改测试跳过。仍有既有 macOS linker unwind 警告。

## Scope
测试覆盖生产 parse_params_str 与 CwdParams 组合；源码确认两入口 ? 位于配置操作前。未对旧 reset 执行真实用户配置清空，也未宣称真实 ACP 写盘回归。未发现仓内调用者，但 MvpAgent 前缀路由可达，故不据此提出删除。CLI 尚未重新链接。
