# Verification

## Root cause and repair

父从用户错误文案定位Pager replay_inherited_updates→公开stream_session_history_at→with_prepared_history→普通id resolver。Windows普通namespace pin不分享DELETE，会与仍有DELETE访问的发布句柄冲突32；包装文案并非证明目录逃逸或非实体。Microsoft CreateFileW官方共享说明经context7核对，双向检查访问权与sharing，不要求是外部进程。

唯一生产变化：with_prepared_history使用open_session_by_id_shared_read。其返回同一validated entity，Timeline/history解析与错误传播保持；read-only capability不入writer cache、不拿lease、不触发恢复/模型。mutation callers和writer pin未改。

## Static review and build

独立luna窄review无生产阻塞，父读取3文件diff与真实入口。当前合并源码（含这1行生产修复）defaultCLI check/build已exit0；新CLI的subagent模型/effort/resume组合3/3和Goal迟到交付1/1通过。构建SHA256与实际命令见[同批集成记录](../2026-11-05-control-running-subagent-model-effort/verification.md#integrated-cli-acceptance)，此组证明集成/调用链，不等同Windows共享机制运行证明。

## Platform boundary

主机aarch64 macOS，无Windows原生运行器或Wine。已安装Windows Rust标准库target不能提供native句柄实验；本轮没有提交/推送或远程CI dispatch。因此cfg(windows) `session::storage::jsonl::tests::child_history_stream_coexists_with_delete_capable_publication_handle` 尚未在Windows编译执行，不能记pass。该exact已接入现有Windows session storage工作流，未弱化既有immutable commit与其他选择。

Windows专项使用真实READ|DELETE publishing handle+shareREAD|WRITE保留冲突，普通pin raw32/普通resolver失败作负对照，公开stream/replay/load才成功；核对有序内容、timeline/summary字节、no cache/lease、关闭发布handle后普通id reopen恢复。no-reparse仍复用既有生产实现，child symlink测试限定Unix，本机通过不当Windows reparse执行证据。

## Portable acceptance

父亲自读取最终完整log `01a1a973.../terminal/call_77bc6643d7404271a720963493650b54.log`。task01a1a97f-0b54-7e73-afb9-8aa2d3aca41e已确认completed/exit0，总43.00秒；Cargo/rustc/rustdoc实际版本均1.93.1，RUST_MIN_STACK16777216，GROW_HOME=/tmp/grow-windows-child-history-window6-5n3v2tv5/home。

每个命令 `python3 scripts/run_exact_cargo_test.py --package shell --lib <full name>` 均先enumerate恰好1项，再实际执行1 passed、0 failed、0 ignored、4589 filtered out：

| Full test name | Actual result |
| --- | --- |
| session::storage::jsonl::tests::response_projection_production_read_only_cores_preserve_unified_timeline | 1/1，公开child stream的session身份/有序canonical内容/bytes不改与lease排他 |
| session::storage::jsonl::tests::shared_read_session_resolution_stays_out_of_the_writer_cache | 1/1，observer不填writer cache |
| session::storage::jsonl::tests::coordination_observer_preserves_live_sideband_and_new_writer_recovers_once | 1/1，live Sideband公开child stream不写恢复，replacement owner只恢复一次 |
| session::storage::tests::stream_replay_updates_at_missing_session_is_empty | 1/1，missing公开stream为空 |
| session::storage::tests::stream_replay_updates_at_surfaces_read_errors | 1/1，真实读取错误继续传播 |
| session::storage::tests::child_history_stream_rejects_invalid_sources_before_emission | 1/1，corrupt-history/Timeline-directory/session-file/identity/duplicate五类来源在零emit时拒绝，bytes保持 |
| session::storage::tests::child_history_stream_rejects_symlinked_session_entities | 1/1，仅Unix链接边界 |

初轮7/7通过后，仅修test新增调用的must_use告警与launcher显式/tmp目录，原筛选最终再次7/7通过，生产1行未变。最终全workspace Cargo fmt --all -- --check与git diff --check均exit0；独立luna静态无生产阻塞，父接受修复与可行验证。Windows native/cross未执行的限制保持，不能据此报告用户Windows现场已恢复。

正常Grow进程、安装与正常session存储未改，Windows现场尚未获得新构建，本页不声称现场已恢复。

## Specification validation

本change建立后独立strict通过，归档前全量strict19/19；与同批child sampling和compaction变更归档合入主规范后，全量strict16/16、全历史archive853/853，0 failed。全workspace Rust1.93.1 Cargo fmt --check和git diff --check通过。父按实际canonical结果接受实现及独立review；归档确认已完成的窄修复和可行本机验证，不把未运行的Windowsnative用例写为通过。
