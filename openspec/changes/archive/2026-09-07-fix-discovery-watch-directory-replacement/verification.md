## 已验证
独立 rustc harness include 实际 crates/codegen/shell/src/config/watcher.rs，没有复制被测实现。配置发现的入口 stub 为 unreachable，本次测试使用显式临时根；project_root stub 返回输入临时根。因此不覆盖真实 project_root / 全量配置来源集成。

旧实现 refresh_detects_atomic_directory_replacement 失败：replacement needs a new watch。修复后全部 10 项通过，6.07s，含真实 macOS OS watcher、config watch 旧回归、注册计划和原子替换。Linux/Windows 未运行。

cargo tree --locked --offline -p shell -i same-file --depth 1 通过；仅新增 Shell 对锁文件既有 same-file 1.0.6 的直接依赖。定向 git diff --check 通过。

## 待完成
完整 Cargo Shell 测试先前因 No space left on device 在 incremental query cache 写入失败。独立测试不能替代完整 crate 集成构建，当前不归档。恢复空间后执行：
```
CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib config::watcher --quiet
```

## 本地 harness
本次临时目录：/var/folders/gm/lzn3ylzx4lz7mx29lftngdz40000gn/T/grow-watcher-check-zf9icmbm

入口：
```rust
#![allow(dead_code)]
mod session { pub mod workflow { pub mod registry { pub fn project_root(p: &std::path::Path) -> std::path::PathBuf { p.to_owned() } } } }
mod watcher {
use notify_debouncer_mini::notify;
mod tools { pub mod util { pub mod grow_home { pub fn grow_home() -> std::path::PathBuf { unreachable!("isolated watcher test uses explicit roots") } } } }
mod agent { pub mod prompt { pub mod skills {
pub fn user_skill_roots() -> Vec<std::path::PathBuf> { unreachable!() }
pub fn collect_skill_config_dirs(_: Option<&std::path::Path>, _: Option<&std::path::Path>, _: &[std::path::PathBuf], _: &[String]) -> Vec<std::path::PathBuf> { unreachable!() }
} } }
include!("/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/config/watcher.rs");
}
```

编译调用（使用现有依赖产物，避免全量 Shell 链接）：
```python
import subprocess
subprocess.run(['rustc', '--edition=2024', '--test', '/var/folders/gm/lzn3ylzx4lz7mx29lftngdz40000gn/T/grow-watcher-check-zf9icmbm/test.rs', '-L', 'dependency=/Users/lordcasser/workspace/projects/grow/target/debug/deps', '-C', 'debuginfo=0', '-o', '/var/folders/gm/lzn3ylzx4lz7mx29lftngdz40000gn/T/grow-watcher-check-zf9icmbm/watcher-test', '--extern', 'notify_debouncer_mini=/Users/lordcasser/workspace/projects/grow/target/debug/deps/libnotify_debouncer_mini-7f5022c0f9149f97.rlib', '--extern', 'tokio=/Users/lordcasser/workspace/projects/grow/target/debug/deps/libtokio-c3771415c85a19b3.rlib', '--extern', 'parking_lot=/Users/lordcasser/workspace/projects/grow/target/debug/deps/libparking_lot-9f6c0808a47485a5.rlib', '--extern', 'dunce=/Users/lordcasser/workspace/projects/grow/target/debug/deps/libdunce-9f0c3aef725577f4.rlib', '--extern', 'tracing=/Users/lordcasser/workspace/projects/grow/target/debug/deps/libtracing-b2fb5b9dee006319.rlib', '--extern', 'tempfile=/Users/lordcasser/workspace/projects/grow/target/debug/deps/libtempfile-080578feaf288190.rlib', '--extern', 'same_file=/Users/lordcasser/workspace/projects/grow/target/debug/deps/libsame_file-7528b8fb9bfe2b1b.rlib'],check=True)
```

## 完整集成验证已恢复
用户授权 cargo clean 后清理 77.3 GiB，恢复约 81 GiB 可用空间。以下结果取代此前“待完整 Cargo 验证”的状态；旧记录保留用于追溯。
完整 Cargo Shell 测试：watcher 10 项通过（6.14s），reloader 4 项通过。实际项目根解析和真实 macOS watcher 均使用生产代码。Linux/Windows 未运行。

执行命令：
```sh
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib config::watcher --quiet
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib config::reloader --quiet
```
现有 ld __eh_frame 大小警告仍出现，但命令返回 0，未视作失败。
