# v2.6.16 发布验收

## 候选依据

正式最新v2.6.14。v2.6.15候选5f0a8bf92通过本地13528项核心库、四项真实PTY、1.93.1 all-targets/fmt和默认CLI构建，但官方两个Windows门禁发现仅Unix测试helper的cfg遗漏，未公开。原run43923085996已取消，失败tag保留且release接口返回404；完整事实见相邻prepare-release-v2-6-15归档。

后继v2.6.16相对该候选只含一行测试资格修复、33个本地继承包的patch同步及文档收尾。既有Unix产品行为证据复用，Windows资格修复须由本版官方原生门禁证明。

## 本地复核

- 父亲读working_snapshot.rs diff，唯一Rust改动为timeline_generation的cfg(test)→cfg(all(test, unix))。tools字段与调用test mod本来就是Unix；未增加stub、假0或运行时fallback。
- 父读Cargo.toml/Cargo.lock完整diff，只有33个本地包2.6.15→2.6.16版本行，外部及独立版本未动。1.93.1 cargo metadata --locked --no-deps退出0。
- 1.93.1全仓fmt退出0，OpenSpec all strict为20 passed/0 failed，归档后14个Markdown文件88处相对链接/requirement锚点检查通过。
- 1.93.1工作快照原回归退出0：51 passed、0 failed、1既有ignored，覆盖目录/suffix generation。该命令在版本递进前捕获2.6.15 metadata，Rust源码已包含cfg修复，测试日志terminal/call_ea21c9f89a2b4649bf1479a6d9b5875b.log。此结果不代替新候选Windows原生门禁。

## 正式发布

尚未提交/tag/运行v2.6.16；以实际完成后的新SHA、官方门禁和公开资产事实补记。
