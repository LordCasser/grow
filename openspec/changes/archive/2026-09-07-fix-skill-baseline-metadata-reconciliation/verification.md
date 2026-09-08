## Results
旧实现 baseline_same_path_metadata_changes_reach_runtime 失败，提示 same-path metadata change must reach runtime。
修复后独立测试 skill_discovery_tracker 过滤范围内 85 passed；明确排除 smoke_table_first_skills_do_not_flatten_into_listing（该测试调用技能解析入口，harness 中为 panic stub，不伪造解析结果）。测试实际编译仓库中的 SkillManager、conditional、listing、SkillInfo、ConfigSource、truncate 文件，未复制被测实现。

测试覆盖同路径 description / enabled 变化、相同数据无 pending、条件技能隐藏/激活/重载、动态技能保留、listing 等。不是 ToolBridge/会话端到端验证。完整 Cargo tools 测试待磁盘空间恢复，当前不归档。

## Reproduction
临时路径：/var/folders/gm/lzn3ylzx4lz7mx29lftngdz40000gn/T/grow-skill-check-i9n_76ge
```rust
#![allow(dead_code, unused_imports)]
mod implementations { pub mod skills {
#[path="/Users/lordcasser/workspace/projects/grow/crates/codegen/tools/src/implementations/skills/types.rs"] pub mod types;
pub mod discovery { pub fn parse_skill_files(_: Vec<(std::path::PathBuf, super::types::SkillScope)>) -> Vec<super::types::SkillInfo> { panic!("parser integration excluded from isolated harness") } }
} }
mod types { #[path="/Users/lordcasser/workspace/projects/grow/crates/codegen/tools/src/types/config_source.rs"] pub mod config_source; }
#[path="/Users/lordcasser/workspace/projects/grow/crates/codegen/tools/src/util/truncate.rs"] mod util;
#[path="/Users/lordcasser/workspace/projects/grow/crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs"] mod skill_discovery_tracker;
```
```python
import subprocess
subprocess.run(['rustc', '--edition=2024', '--test', '/var/folders/gm/lzn3ylzx4lz7mx29lftngdz40000gn/T/grow-skill-check-i9n_76ge/test.rs', '-L', 'dependency=/Users/lordcasser/workspace/projects/grow/target/debug/deps', '-C', 'debuginfo=0', '-o', '/var/folders/gm/lzn3ylzx4lz7mx29lftngdz40000gn/T/grow-skill-check-i9n_76ge/skill-test', '--extern', 'serde=/Users/lordcasser/workspace/projects/grow/target/debug/deps/libserde-9478072046821ae4.rlib', '--extern', 'serde_json=/Users/lordcasser/workspace/projects/grow/target/debug/deps/libserde_json-ca8a08c274d9da5e.rlib', '--extern', 'strum=/Users/lordcasser/workspace/projects/grow/target/debug/deps/libstrum-435266f018f6a444.rlib', '--extern', 'dunce=/Users/lordcasser/workspace/projects/grow/target/debug/deps/libdunce-9f0c3aef725577f4.rlib', '--extern', 'tempfile=/Users/lordcasser/workspace/projects/grow/target/debug/deps/libtempfile-080578feaf288190.rlib', '--extern', 'ignore=/Users/lordcasser/workspace/projects/grow/target/debug/deps/libignore-eea931d3719e9a66.rlib', '--extern', 'tracing=/Users/lordcasser/workspace/projects/grow/target/debug/deps/libtracing-b2fb5b9dee006319.rlib', '--extern', 'token_estimation=/Users/lordcasser/workspace/projects/grow/target/debug/deps/libtoken_estimation-dfdadda8756bc60e.rlib'],check=True)
```
运行 skill-test skill_discovery_tracker --skip smoke_table_first_skills_do_not_flatten_into_listing。

## 提示效果补充验证
扩展 baseline_same_path_metadata_changes_reach_runtime 后再次独立编译实际源文件，定向 1 项通过。断言 send_available_commands=true；description 修改出现在新的 system_reminder；唯一技能停用时 system_reminder=None；相同数据再次重读仍无 pending。此次仅新增断言，没有再次修改生产逻辑，之前 85 项覆盖保持为前一轮结果，不计作本轮全量重跑。历史提示不被改写，未声称运行中的模型已撤回旧信息。

## 完整集成验证已恢复
用户授权 cargo clean 后清理 77.3 GiB，恢复约 81 GiB 可用空间。以下结果取代此前“待完整 Cargo 验证”的状态；旧记录保留用于追溯。
完整 Cargo 构建成功。tools 实际运行 86 项全部通过（包含真实解析集成入口）；shell 在该过滤器下运行 0 项，只作为编译验证，不计作测试覆盖。

执行命令：
```sh
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p tools -p shell --lib skill_discovery_tracker --quiet
```
现有 ld __eh_frame 大小警告仍出现，但命令返回 0，未视作失败。
