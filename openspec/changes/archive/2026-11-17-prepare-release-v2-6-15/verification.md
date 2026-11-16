# 发布验收

## 候选与授权

用户于2026-11-17要求更新文档、闭合当前遗留并release最小版本；本次为v2.6.14→v2.6.15。当前本地和远端main基线均为733d2c72ed0436dc27f050ee2149dd41432be552，既有恢复功能验收详见已归档repair-session-resume-presentation。

旧prepare-release-v2-6-5已回溯核对真实公开发行、annotated tag及11项资产后归档。当前收尾仅包含本轮已识别gap锚点、格式、版本、文档和CI登记，不启动明确延期backlog。

## 工具链与检查

功能验收阶段默认PATH为Homebrew Rust/Cargo1.99.1；发布流程固定1.93.1。发布前检查显式使用`PATH="$HOME/.cargo/bin:$PATH"`，保持rust-toolchain.toml不变，不能以1.99结果代替1.93兼容性。

- 发布/exact-runner Python静态契约测试退出0；四项新恢复PTY按真实target/name登记到core-regression，发布与main push将执行。
- Git HTTPS一次HTTP2 framing失败；单次`git -c http.version=HTTP/1.1 ls-remote`成功，未修改持久Git配置。后续发布传输保持同一显式选项。

- `cargo metadata --locked --no-deps --format-version 1`在1.93.1下退出0：CLI为2.6.15，逐包核对Cargo.lock恰有33个本地继承包版本变化，无外部或独立版本变化。
- `cargo check --locked --workspace --all-targets`在1.93.1下退出0。首轮发现`test_subagent_orphan_reconcile`的Spawn fixture缺新增`parent_prompt_owner`，已补显式None，保持无Turn归属的孤儿恢复前提，未放宽产品规则。
- `cargo fmt --all -- --check`在1.93.1下退出0；原28个路径的格式遗留已收敛，机械整理前后非格式token数量保持。新gap回归和nav注释同步后再次全仓通过。
- 已删除history_window中未被调用的两项旧替换接口，Around继续使用原apply_history_page的原子采用/失败回滚路径；没有新增替代安装路径。

- 本轮39个Markdown文件的542处相对链接/requirement锚点检查通过。归档gap与旧2.6.5准备任务后，`openspec validate --all --strict --no-interactive`为19 passed/0 failed，全部archived验证退出0。
- 固定1.93.1的核心组合测试整体退出0：ChatState676、Pager7719、Minimal104、Shell4867、tool-types162，合计13528 passed、0 failed、32项既有ignored。gap的三个新用例、正文夹紧和follow gap保护均通过，独立delta已归档。日志为本session terminal/call_7ae2a06ffa28403e807530a23fa5e862.log。
- `cargo build --locked -p cli --bin grow`在1.93.1下退出0；隔离GROW_HOME运行version/help成功，version显示grow2.6.15及当时未提交的基线733d2c72e。正式发行由workflow绑定后续tag commit，不以此默认构建替代release-dist产物。

- 同份1.93.1默认CLI的四项真实PTY全部通过：多页Plan/Behavior与消费不变、100+50同Turn挂起/冷恢复（2 passed，terminal/call_197392c3d4704003872c4907faf10933.log）；Minimal浏览/退出/再次恢复和目录未加载目标直达/返回live（exact各1 passed，terminal/call_82e1b2e48a0b4e43b5059f5a2594a0f1.log）。四项已登记官方core门禁；本机mock终端通过不宣称任意模型或所有OS原生行为覆盖。

## 正式发布

- 正式候选提交`5f0a8bf92c5a1c13136f06be6902969fdedf8a5d`：Release Grow v2.6.15 with reliable session resume presentation。
- 在干净已提交候选建立annotated `v2.6.15`，固定1.93.1执行`bash scripts/validate-release.sh v2.6.15`退出0，核对tag/HEAD/workspace/notes及十资产发布契约。
- 使用显式HTTP/1.1原子推送main与tag成功，无force或历史重写。默认main dispatch所有平台组和publish=true。
- [官方run43923085996](https://github.com/LordCasser/grow/actions/runs/43923085996) 已启动，headSha确认为上述完整SHA，event为workflow_dispatch；矩阵和输入检查成功。后续门禁、构建和公开资产以该运行完成事实为准。

## 候选结论：未发布，转交v2.6.16

官方Windows两个必需门禁在编译shell lib tests时均失败：working_snapshot.rs:882的timeline_generation仅有cfg(test)，却访问IndexedHistory在Unix才有的tools字段，错误E0609。两个caller测试均为cfg(all(test, unix))，这项macOS无法暴露的测试平台资格遗漏由fix-history-index-test-cfg单独闭合；不是授权忽略Windows测试的依据。

- Windows native协调job133495510825和storage job133495510856的原始日志均确认相同编译错误；没有进入对应测试行为验收。
- 已请求取消本次不再能发布的run，最终status=completed、conclusion=cancelled，两个失败job保留failure。其余取消项不计通过。
- GitHub release-by-tag v2.6.15返回404，未产生公开或draft release。annotated tag仍指向5f0a8bf92，未删除、重打或force push。
- 下一个未占用patch v2.6.16已确认tag不存在，将由prepare-release-v2-6-16重新执行完整正式门禁、构建和公开资产验收。本候选以失败尝试归档，不宣称正式发布完成。
