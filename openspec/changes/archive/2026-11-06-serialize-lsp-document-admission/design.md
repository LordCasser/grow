# LSP document notification and version boundary

## Evidence and ownership

`LspManager::notify_file_changed`调用`LspClient::notify_file_change`，后者`Documents::plan`→`ServerSocket.did_open/did_change`→`Documents::commit`→didSave/pull→manager pending。publish router在另一runtime线程调用`documents.version(uri)`，再由`DiagnosticsStore::record_push`把reported截到latest_sent。版本读发生在入队成功与commit之间时，版本化push会被误归旧文本，无versionpush也会误归旧文本；收到的报告不会再次归属，pending可等到超时。

亲核锁定依赖async-lsp0.2.4的`ServerSocket::notify`（lib.rs689–697）：Ok仅表示消息入队，调用同步且不执行响应回调。不能称此刻已抵达server，也不能把本地版本先推进后遇发送失败吞掉。

## Decision

沿用Documents的现RwLock。新增/替换一个模块内同步入口，概念签名为`send_update<E>(&self, uri: &str, language_id: &str, end: Position, send: impl FnOnce(Update) -> Result<(), E>) -> Result<i32, E>`。它在一次write guard下根据当前Tracked计划Update、调用仅做同步通知入队的closure；Err原样返回且不改Tracked，Ok写入version/end后释放guard。客户端生产入口改用它，didSave与pull仍在成功提交且guard释放之后执行。局部命名及等价签名可由coder选，不新增guard/token/transaction实体。检查并移除仅为旧生产二阶段API而存在的死入口；测试必要初始化可以用cfgtest helper，不保留新的生产旁路。

响应侧沿已有version读取共享锁，入队完成但提交未释放时必须等；同一连接的更新计划也不再从过期Tracked计算。同步closure不得读取Documents或等待响应，避免重入死锁；读者在锁释放后才拿DiagnosticsStore锁，不能反向持有diagnostics锁再等Documents。当前notify实现只入队，满足此前提。只串行化微小本地通知入队，不持锁跨await/进程IO/网络roundtrip。

## Invariants and rejected alternatives

失败didOpen保持unknown，失败didChange保持旧version、language和end，下一次range仍基于最后成功入队文本。firstVersion、UTF16端点和增量full replacement不改。旧版本报告不能结算新edit，未打开push不能结算firstedit，未来版本仍不得冻结store；不删除record_push的version clamp。已观察push的server不再用pull空报告覆盖；不改drain2s测试为更大等待、不sleep重试原用例掩盖缺报告。不把版本提前发布后rollback：独立读者在rollback前已可能观察失败文本。

## Verification and parent acceptance

先证明入队后/commit前的受控窗口在旧路径可导致pushcovers旧version，新路径不能；覆盖didOpen和didChange、versioned与versionless实际归属、失败发送不推进且range保持。同步channel/barrier控制时序，有限watchdog只防挂死，不用随机stress当唯一证据。必要测试seam仅cfgtest，不留生产delay。原mixed push/pull测试及相邻stale-version/preopen/incremental/failure控制保留。运行tools的LSP定向组、固定1.93.1 fmt/warning检查即可；全Core留精确提交CI，父读diff/真实redgreen/相关入口验收，不再派新独立review。若实验证明CI另有原因，保留该差异、只修已证竞态，不静默扩scope。
