# AC-08 ordinary update persistence verification

日期：2026-11-08。实现提交 `bac1f546`；集成源码 `ed2027c0` 的相关 application、ACP/slash caller、CLI caller 与测试文件逐一 diff 相同。测试使用 Rust 1.93.1、Linux、本地 bare Git remote 和隔离 Grow home，没有在线模型依赖。

## 真实入口与事务回归

- `cargo test --locked -p shell --test test_plugin_install_persistence --test test_acp_plugin_update_results -- --test-threads=1 --nocapture`：Update integration **2/2**，Install/Uninstall integration **5/5**，退出 0。分别用时 0.81s、0.83s。
- `cargo test --locked -p pager --lib plugin_cmd:: -- --test-threads=1`：**10/10**，退出 0，1.06s。其中 public dispatcher 回归在独立进程中覆盖 uninstall/update 的 save、parse、read、正常与 no-op/异常 path；既有 Install 与 marketplace fresh-cache 入口同时通过。
- `cargo test --locked -p plugin-marketplace --lib transactional_update -- --test-threads=1`：**5/5**，退出 0。保存失败 rollback、既有 artifact/registry 保留、时间戳、local recopy 与 traversal 拒绝保持。
- Shell `cargo clippy --locked -p shell --lib --tests --no-deps`：退出 0，现有 warnings 未作为无警告声明。

ACP 验证经真实 extension→session handle→actor：坏 registry 返回 InternalError 且不拉取 Git；save fault 时 checkout 已到新 tip、registry 旧字节不变，错误包含 repo/path/不完整事实，reload/restart 均 false。移除故障后显式重试正确采用 commit、更新时间与新增 plugin map，随后同 tip 在 save fault 仍开启时保持成功 no-op 和原 registry 字节。pinned 与 local 也不触发普通保存。

混合批次按 ordinary→marketplace→ordinary 排序。通过既有 `GIT_BIN_PATH` seam，在最后一个 repo 的真实 pull 前保留 marketplace 已提交的 registry，并以同名目录阻塞最终 registry rename。该回归确认：marketplace v2 artifact/record 已提交；先前 ordinary checkout 已推进但 durable record 仍旧；末尾保存失败明确返回错误。解除故障后再运行完整批次，ordinary commit/plugin map 收敛且 marketplace 结果保留。没有修改 marketplace 内部事务或加入生产 fault hook。

## 窄范围有效旧红/新绿

`run-baseline-control.py` 从基线 `8fbc52185ce30e38d97745dd6597242da1135c31` 提取完整原始 `crates/codegen/shell/src/plugin.rs`，不编辑其内容；与 `baseline-control.rs` 一起链接既有生产 Shell rlib 及其精确 fingerprint 依赖。旧模块与当前 `shell::plugin` 使用相同夹具和真实 git/install registry/save fault。

```sh
python run-baseline-control.py --shell-rlib /path/to/debug/deps/libshell-HASH.rlib
```

本次使用 `libshell-25ec39aa2e19cbee.rlib`。三组旧控制均退出 101，三个新控制均退出 0：

| 场景 | 旧行为实际证据 | 修复后 |
| --- | --- | --- |
| ordinary update save 失败 | checkout 等于新 tip，registry 旧字节不变，仍返回 `Ok(())` | 返回不完整登记错误，保留相同真实磁盘事实 |
| uninstall save 失败 | artifact 已不存在，registry 旧字节不变，仍返回 `Ok(())` | 返回已删除但未注销的部分完成错误 |
| uninstall unexpected file | artifact 仍存在，registry 字节改变且记录被注销，仍返回 `Ok(())` | 返回错误，保留文件与原 registry 字节 |

这是共享 application 的源码级差分运行，不是完整旧版 Shell、ACP 或 CLI 二进制重建。当前 ACP 和 CLI public dispatcher 的运行证据来自上面的独立 Cargo 回归。

## 审查与资源边界

独立数据完整性审查指出，提前把 ordinary 结果写入内存 registry 会被后续 marketplace save 顺带发布，令末尾冗余 save 的失败错误归因。修复后保留完整 pending UpdateResult，待各 repo 操作完成才采用到最新 registry 并保存；审查复核该顺序和重试边界，无新增问题。旧 slash caller 的 operation failure 也改为其既有 error 事件；未以此声称完成独立 slash UI 场景覆盖。

早期完整 Shell lib-test 编译两次被外部终止（137），测试未运行。最终上述 integration/Pager 测试用单编译 job、lld、命令行 `profile.test.package.shell.codegen-units=512` / `profile.test.package.pager.codegen-units=512` 完成；没有降低断言，也没有修改仓库 CI/profile。该记录不等于全库测试或所有远端 CI 已绿。

## 最终独立 CLI

同次集成源码 `ed2027c0`（tree `bf0f734a`）的独立 `grow` 生产构建成功，退出 0；使用 Rust 1.93.1、lld、单 job 与命令行 dev.package.shell/pager CGU512，未改仓库 profile。`python cli-smoke.py /path/to/grow` 的 **11 次独立 CLI 调用全部通过**：

- 卸载 save 部分完成返回 exit 1，registry 旧字节保留；显式重试 exit 0 完成注销。
- 非预期文件与坏 registry 返回 exit 1、保原证据；多插件未确认不删，确认后成功。
- 更新 parse/read 错误先于 Git 变化；save fault 时 HEAD 已推进而 registry 保旧，exit 1 且不打印 Updated。
- 移除故障后 retry exit 0，commit 与完整新增 plugin map 正确；同 tip 在 save fault 开启时仍 exit 0、字节不变。

CLI smoke 总 runner 退出 0。生产 binary 与上述 library/ACP 回归使用同一份相关源码；此处仍不宣称全量远端 CI 完成。


## 规范与归档

OpenSpec 1.11.0：归档前 all/strict 21 项通过；两个契约正常归入 `configuration-rules`，工具生成的归档目录日期为 2026-11-09。开发说明和 backlog 已同步。最终 all/strict **19/19**、archived **925/925** 通过，`git diff --check` 通过。首次归档复核只提示最后的归档复核任务尚未勾选；完成本步后重新运行，两项任务清单均 5/5。此规范验收不替代远端完整 CI。
