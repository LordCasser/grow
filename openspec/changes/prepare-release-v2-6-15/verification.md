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

候选本地验收已通过；提交、tag、官方运行及资产结果将在实际完成后追加。
