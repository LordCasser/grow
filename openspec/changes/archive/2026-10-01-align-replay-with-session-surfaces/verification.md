# 验证记录

## 构建与测试

macOS/aarch64，2026-09-30 至 2026-10-01。先按默认 profile 验证，磁盘空间不足导致一次 Pager query-cache 写入及一次 Shell LLVM 输出失败；清理本轮对象/增量目录并执行 `cargo clean -p shell -p pager -p cli` 后，关闭增量并只对这三个包关闭 debug 信息重建。配置仅通过命令行传入，不改仓库 build 配置。最终命令共同前缀：

```sh
CARGO_INCREMENTAL=0 cargo \
  --config 'profile.dev.package.shell.debug=0' \
  --config 'profile.dev.package.pager.debug=0' \
  --config 'profile.dev.package.cli.debug=0'
```

| 验证参数 | 结果 |
| --- | --- |
| `build --locked -q -p cli --bin grow` | 构建通过；macOS linker 的 __eh_frame 大小警告不阻止构建。 |
| `test --locked -q -p pager --lib replay_cmd` | 42 passed：包含真实 tree/browser/panel/clock/idle 模块；深浅主题共享 task row、future gate、mouse child/返回、resize 40×8/20×4/1×1 buffer、Finished 无效控制。 |
| `test --locked -q -p pager --lib transcript_projection` | 18 passed：source elapsed、terminal 后 late progress、打断后追加输入/旧终态、Goal clear、Workflow clear 和 schedule delete。 |
| `test --locked -q -p pager --lib views::tasks_pane` | 53 passed：正常管理控件保留、只读行省略 kill/manage、h 完成项。 |
| `test --locked -q -p pager --lib views::turn_status` | 44 passed：普通输入提示保留，只读阻塞状态保留任务数量且无输入/执行动作。 |
| `test --locked -q -p pager --lib app::subagent::tests` | 42 passed：正常 child 延迟历史、父消息及去重。 |
| `test --locked -q -p pager --lib export_cmd::tests` | 6 passed。 |
| `test --locked -q -p pager-minimal --lib` | 94 passed；keyboard-only normal host 仍有输入提示。 |

## 真实 PTY

使用本轮构建的 `target/debug/grow replay 01a0eaf7-3acf-7872-8084-f4432d1c57a2 --speed 16/4096`，经 ptyctl 的真实 PTY 输入和 screen cells 检查：

- 120×36：普通顶栏、自动展开的 3 个进行中 subagent、历史活动行、三行 composer 槽位控制框和单次 ShortcutsBar。无输入框、kill 或 stop 按钮。
- F8 后 Playing→Paused；历史时刻停在 `2026-09-29 10:32:32 UTC+08:00`，子 Agent elapsed 多次检查均为 `6m51s`，回放位置 `11:19` 不变。共享状态行中的 queue/steer 提示由验收发现并修正，44 个 turn status 测试覆盖只读结果。
- 普通任务列表 Down/Enter 进入 child `01a0eafb-6bc1-78f3-b1aa-ae817d8e59a7`，可见该节点正文和工具；独立 Esc 返回 root，保持列表和阅读状态。Finished 用 `t` / `h` 查看完成项。
- Enter 打开既有 Details viewer；`?` 打开含 Session、历史时区、最后 IDLE、Behavior/Goal、未决工具和时间精度的只读帮助；局部 Esc 返回。正文 `/Goal` 搜索显示 32 matches，快捷栏切换为搜索提示，输入不会发送到 session。
- 40×8 与 20×4 保留正文和帮助/退出控件；20×4 Finished 写“完成”，正常结束后仍可浏览。
- Ctrl-C 后 PTY 状态为 alt_screen=false、bracketed_paste=false、mouse_reporting=false、show_cursor=true。Replay 来源 summary/updates/timeline 三文件检查前后 SHA-256 全部一致。
- 一次 `/usr/bin/time -l` 完整浏览运行 maximum RSS 为 528,957,440 bytes（约 504.5 MiB）、peak footprint 487,703,512 bytes。248.44s wall 包含人工/自动浏览等待，不作为加载耗时。该来源与会话树规模固定，不推导任意大来源的内存上限。

## 范围与限制

1×1 的 Ratatui buffer 和控件边界测试通过；真实 PTY 的 1×1 resize 会令 ptyctl HTTP helper 阻塞，观察到 helper 高 CPU、grow 低 CPU，因此不把该工具路径记为真实终端验收通过。已记录为工具限制，不扩入本次产品修复。真实窗口验收最小为 20×4。

只读 Replay 不真实恢复来源 Goal 或运行历史工具；缺失实时 stdout、model/context 等不能补取。现有额外信息读取、clipboard 明确动作与外链禁止边界沿前序 interactive-readonly-replay，不新增执行入口。

OpenSpec 的逐项 delta、审计与开发者说明在本 change 核对，最终 strict/archive 校验结果见本记录末尾。

## OpenSpec 完成检查

归档前 `openspec validate --all --strict --no-interactive`：16/16 passed。两项 change 归档后主规范 strict：14/14 passed。首次 archived 校验仅因本次归档收尾任务尚未勾选而报告两项未完成；完成核对后再校验：570/570 passed，所有 tasks 已完成。`git diff --check` 通过。

最终格式检查：19 个修改/新增 Rust 文件执行 rustfmt --check（skip_children=true）通过；仅检查本次文件，未重排无关既有测试。验收完成后执行 cargo clean -p shell -p pager -p pager-minimal -p cli -p ptyctl-cli，清理 3.8 GiB；当时磁盘剩余约 22 GiB。验证阶段未替换已安装的 grow。
