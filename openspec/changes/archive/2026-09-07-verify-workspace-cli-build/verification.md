## Results
- main 工作区执行 `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo build --locked --offline -p cli --bin grow`，退出 0，耗时 3m33s。
- 存在 macOS linker 的 __eh_frame 超过 16MB compact unwind 编码警告；未阻止链接，不视为已解决。
- `target/debug/grow --version` 退出 0：`grow 2.1.4 (1e1fda6d) [stable]`；`--help` 退出 0，输出 6129 bytes；二者 stderr 均为空。版本哈希不是未提交修改的唯一标识。
- 二进制 458945840 bytes。target 从 6.5 GiB 增至 9.8 GiB，可用磁盘从约 72 GiB 降至约 69 GiB；保留本轮有效构建缓存，后续持续监控。
- 不替换 ~/.local/bin/grow；本次不包含交互 UI、真实 provider 或用户已有会话恢复验证。
