# 最终验收

父已核对修正后生产实现、公开入口回归、独立Luna复核和新冻结版本的全部实际结果，WS-03单项验收通过。普通删除与后置stale清理均由fast-worktree共同owner验证common Git目录、真实直接registration、commondir和exact checkout/.git backlink；未验证指针不会扩大删除权，失败保留metadata与可重试事实。原32项普通API证据不能独自闭合后置helper的漏链，本轮补上12项并确认旧实现反例失败后才接受。

实际执行`python3 /tmp/grow-ws03-stale-verify.py`，固定Cargo/rustc/rustdoc1.93.1、RUST_MIN_STACK16777216、JOBS2、macOS27.0.1/UID501、独立/tmp GROW_HOME/TMPDIR和空Gitconfig/hooks。构建命令为`rustup run 1.93.1 cargo test --locked -p fast-worktree --features metadata --lib --no-run --message-format=json-render-diagnostics`；从产物确认每个完整名只枚举一次，再分别`--exact --nocapture --test-threads=1`执行。`/tmp/grow-ws03-stale-green-nrmtgewv/results.json`逐项记录44个不同测试，全部exit0、每项1 passed/0 failed/0 ignored、权限fixture跳过0。父已读回全44项清单、runner的判定逻辑及public rehydrate两项原始日志。

`python3 /tmp/grow-ws03-stale-verify.py --old-helper-red`使用真实独立源码`/tmp/grow-ws03-stale-redsource-nrbt7k`和独立target，保持同一新测试与其他修复，仅恢复旧worktree.rs（hash `8a9e138f08fba20c526b438a1c155efd8948ac48d35a721c6e521731fe6b1ad8`）。编译成功；Path、Prefix和public rehydrate三项均恰好0 passed/1 failed/0 ignored、exit101。父读回`/tmp/grow-ws03-stale-red-jpszoh0v/test-00..02.log`：前两项在误删计数left1/right0失败，rehydrate在错误成功导致未保留registration的断言失败，不是编译或环境错误。两批runner本身exit0，表示全部期望成立。

| Delta场景 | 实际证据 |
| --- | --- |
| 合法linked删除与兄弟/common保留 | remove_linked_preserves_common_repository_and_sibling；合法scoped/rehydrate对照 |
| separate/forged gitfile不授权目标删除 | remove_separate_git_directory_preserves_metadata；remove_forged_gitfile_preserves_unrelated_directory |
| 相对gitdir/commondir/backlink | remove_resolves_relative_gitdir_commondir_and_backlink；remove_then_stale_resolves_relative_source_and_registration_references |
| registration清理失败可观察 | remove_registration_failure_and_missing_checkout_retry_keep_tracking_until_complete；inspection/checkout/unlink权限错误对照 |
| 后置stale保留已拒绝entry | remove→Path/Prefix foreign commondir、entry/worktrees symlink、不完整/不可读身份；public rehydrate拒绝对照；旧helper三个red |
| 缺checkout的metadata严格重试 | source common身份、缺registration确认、mismatched身份和source不可用保持tracking |

父最终重算四文件SHA256与测试前后freeze完全一致：api.rs `8334c34f501af9c5479b038ac7e65e50f1b8e3c7dd92784d9596dcac6568fa93`；git/worktree.rs `ce2b292a2472cc7f5b94eaa584f53a1264a3a724dba3648fab79b679e58b0317`；git/discovery.rs `eacb0d665145def336f6ca0ffb731f3d2053352b3098dc9a82645bd3cd510af8`；git/checkout.rs `9b68b78be98e13a08f7b205cef0403f8fb78b5af2e85eab307e51bf8183fd6ed`。限定格式和diff空白检查通过。

父已静态核对Workspace force=false错误传播、force=true仅受控Git CLI fallback、取消/subagent清理调用共同API，以及Shell failed resume与rehydrate后置清理使用现共同stale owner；本次以真实公共rehydrate和复合公共API运行验证该根因。Shell自身pristine gate未单独运行，不宣称其mismatched fixture已通过；Windows和真实Linux/btrfs未执行，既有overlay/btrfs身份协议不在本次改写范围。数据动作整体里程碑仍有WS-06a等未闭合项，不因本单项通过宣称backlog整体完成。未操作用户真实工作树、正常Grow存储，未提交/安装/重启。

归档完成后`openspec validate --all --strict --no-interactive`为20 passed/0 failed，`openspec validate --archived --no-interactive`为857 passed/0 failed，其中包含本change。开发说明和backlog闭合引用已同步；归档不表示Git提交或产品发布。

## 历史审查发现：stale 清理绕过（已在最终版本修复）

Luna review `01a1aa10-fb37-7863-b1f4-b0894a34c9f4`最终指出一个阻塞问题。父读回旧`remove_stale_worktree_registrations`源码和Shell失败清理、fast-worktree linked rehydrate调用点，确认旧helper仅验证backlink、缺失路径和匹配范围，缺少commondir同common的验证；它可在普通remove拒绝清理某entry后再次删掉该entry。此结论为代码路径证据，尚未运行新的复合fixture；Shell自身pristine gate的实际通过也未以本次32项证明。

前述32项实际在macOS/Darwin、UID501运行；独立review未重跑。Windows和真实Linux/btrfs未执行。三个冻结hash未变，但此版本不予最终验收，tasks与backlog保持开放。父已在同change修订设计/delta，把后置stale删除也收敛到fast-worktree现owner的一致身份验证；不改正常会话或用户工作树。

## 修正版本的独立静态复核

Luna续审`01a1aa35-ef10-7240-a0c1-0712cc285310`读回修正的stale owner与公开复合调用测试，未发现新的可行动问题。父也已逐行读该生产helper：真实common实体、worktrees/entry拒symlink、checked路径规范化、复用registration_matches_worktree，读取错误保留；best-effort计数与strict metadata重试保持分工。复核确认冻结清单四文件hash，api只加新测试、checkout/discovery未被本修正改写。

该复核阶段仅判定修正静态可接受。9项Shell父计划测试结束后，父将独占Cargo交给`01a1aa3d-31e1-73c3-a51d-77a2f2c9a279`执行原32+新12 exact与独立旧helper三项red；在结果返回前未勾选验收。之后的实际通过和最终验收见本文首节。公开rehydrate回归覆盖同一旧helper根因；Shell pristine gate未运行以及Windows/真实Linux btrfs未执行的限制保留，不把原静态触发链当作Shell实测。

## 历史普通删除候选验证

当时仍待独立review及最终父级验收，不以coder被宿主取消的终态当作实现验收。用户新报session故障优先处理，候选被保留。此阶段证据属于修正前版本。

父已读回第8串行窗口的原始记录：`/tmp/grow-ws03-window8-verify.py`，会话终端记录 `call_97e0a5beddd14a45ba56403ee8dc22de.log` 明确记录构建成功及最终32项通过。构建 `rustup run 1.93.1 cargo test --locked -p fast-worktree --features metadata --lib --no-run --message-format=json-render-diagnostics`；再从产出binary精确执行18项public removal与14项正常/registration回归，合计32个不同exact tests，32通过、0失败、权限fixture跳过0（UID501）。独立GROW_HOME、TMPDIR与空gitconfig，原始分项记录 `/tmp/grow-ws03-window8-de6zvzqa/results.json` 及同目录test-00..31.log；总耗时63.46s。未测试Windows/btrfs真实平台。

父于本次读取日志时重算候选SHA256，匹配测试前冻结指纹：

- `src/api.rs`: `ee91304f7a55650446a3f1b2e8fb7861ea687f8559f1ef05b2373a01cc239223`
- `src/git/worktree.rs`: `8a9e138f08fba20c526b438a1c155efd8948ac48d35a721c6e521731fe6b1ad8`
- `src/git/discovery.rs`: `eacb0d665145def336f6ca0ffb731f3d2053352b3098dc9a82645bd3cd510af8`

公共入口覆盖：正常linked与separate git、checkout/registration symlink、forged/mismatched gitfile/commondir/backlink、相对路径、common repository/sibling保留、inspection/checkout/unlink/registration失败、metadata记录保持与missing-checkout重试。基线还覆盖同一source common身份、source不可用/缺registration的明确处理。测试通过仅接受为该冻结候选的运行证据，尚不清除backlog或勾选整体acceptance。
