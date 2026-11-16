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

- 候选提交`bb9b4c995794db318288beddb00a2cad27e16e70`（Release Grow v2.6.16 with platform-correct history index tests）。
- 干净树创建annotated v2.6.16，固定1.93.1的`validate-release.sh v2.6.16`退出0。main与新tag已用HTTP/1.1原子推送，失败tag未改写。
- 从main dispatch所有平台组和publish=true，官方[run43925904022](https://github.com/LordCasser/grow/actions/runs/43925904022)已启动。
- Windows storage job133505405604和Linux原生协调job133505405607已成功；Windows native已编译并通过Shell local_ipc/coordination，后续场景继续运行。
- macOS native job133505405581在排队约15分钟后被取消，runner_name为空、steps为空。GitHub check annotations明确为“The job was not acquired by Runner of type hosted even after multiple attempts”，并说明macOS arm64容量受限。不是源码或测试失败，不修改16版本/tag。
- 当时同job的重跑请求被API403拒绝，原因“The workflow run containing this job is already running”；后续源码门禁也失败，因此未采用该重跑路径，最终结论见下节。

## 新发现与当前结论

本候选仍未公开；已有源码相关失败，须修复后形成新候选，不能只重跑macOS排队job即宣称通过。

- Windows native job133505405496整体成功（18:35:31Z），与storage成功共同完成cfg修复的原生验收；该独立change已归档。
- Windows x86_64 job133505405480、ARM64 job133505405278正式release-dist编译均只报告相同两项-Dwarnings错误：finish_display_page、prepend_entries_from无生产caller。remove-unused-history-page-paths已删除它们，保留真实prefix gap回归；1.93.1正式profile -Dwarnings check退出0，gap4项/projection22项/fmt通过。该纯清理已独立归档。
- Core job133505406714的完整核心/展示库、Settings235项、默认CLI构建、三个协议恢复场景及失败之前的PTY均通过；在原有ctrl_d_resume_restores_interrupted_command_as_terminal_in_tasks失败。Timeline已结算、正文Taskfailed可见，但Tasks历史为空。定位为分页捕获跳过旧任务状态replay，fix-paged-task-history将独立恢复同cut Tasks中心状态，不放松原PTY oracle。
- Linux五类资产、macOS两类资产和OHOS构建全部成功；Windows两个分发构建失败，其余门禁结果如上。run43925904022最终completed/failure，Publish release assets为skipped。
- 工作流完成后再次查询`repos/LordCasser/grow/releases/tags/v2.6.16`得到HTTP404，无公开release。没有可核对的公开资产，故未执行下载checksum/attestation和原生公开binary验证，不记录为通过。

此失败候选不再发布；保留已推送annotated tag及其原SHA，不重写历史。后继patch只在本轮修复与相关验收完成后确定，按同一正式全平台门禁重新发行。
