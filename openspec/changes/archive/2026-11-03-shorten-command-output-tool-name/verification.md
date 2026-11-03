# 验证记录

本次在既有dirty工作树中仅更名当前源码和开发说明，不改历史archive或业务会话。

- 当前源码、测试和开发说明中，将工具ID/默认模型名统一为get_command_output，将Rust类型统一为GetCommandOutputTool；模块文件未搬迁，参数、权限、命令backend与等待逻辑未改变。
- 旧长名仅保留于registry负断言；没有注册别名。现有registry测试增加真实definition发现、新名dispatch、旧名不可发现/调用检查；最初编译发现新增dispatch测试需要Arc<FinalizedToolset>，已按实际公共API修正夹具。
- 复用共享target，CARGO_BUILD_JOBS=2与开发指南规定的RUST_MIN_STACK=16777216；不创建另一份cargo target或清理其他任务共用产物。

最终构建、定向回归及规范校验结果在执行后补记。

## 最终结果

- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test -p tools -p agent -p shell -p pager -p tool-types --lib --no-run`通过，覆盖更名后的跨包引用；现有deprecated与macOS debug linker警告没有在本次修改。
- 该次构建的测试binary：tools registry 64、command output 12、background_hint 3；agent config 60、builder 34、prompt/template 29；Pager tracker 168、turn activity 13；Shell notification 33、parallel dispatch 13，全部通过。
- registry现有公共入口回归确认新名definition、Read权限与实际dispatch，旧长名不发现也不dispatch。命令输出既有测试保留running/completed、共享期限、空ID拒绝和不回退subagent backend的覆盖。
- `git diff --check`通过；当前crates/docs旧名仅剩3处明确负断言，没有旧工具类型或别名。未执行付费模型评测、全workspace测试或重启业务进程。
- 归档已成功合入session-timeline；归档后全量strict校验17 passed、0 failed，archive校验825 passed、0 failed，最终diff-check通过。
