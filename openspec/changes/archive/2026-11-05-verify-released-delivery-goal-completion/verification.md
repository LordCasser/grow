# Verification

状态：新增隔离真实CLI测试已通过，父已读取最终测试和运行日志、核对文件blob/被测binary身份。此次不改变产品契约，skip_specs；正常会话的旧任务未被恢复或篡改。

## Actual entry and result

新增 `crates/codegen/shell/tests/test_released_delivery_finalization.rs`，最终blob `70b446b1db4a0351514e421282486456810c58f6`。通过host /goal set启动，native todo_write建父step、spawn_subagent创建required background child；真实Task输出取得ID，提交Released(None)后编辑父step，首次native update_goal Complete返回精确两项：

```text
required task <child> has an unhandled parent update
released task <child> still needs explicit execution and wait disposition
```

随后native kill+wait严格得到Cancelled/all_finished；父Spawn/Ended与子Seed/Result及planning resultRef exact linkage，历史Released(None)原样且无Accepted，pending replan成为Unapplied，最终native update_goal Complete和精确多行报告持久化。退出自建writer后冷读仍通过。child为01a1a94e-f357-7271-b5c4-41e3cc892a98。

实际测试输出：1 test；1 passed，0 failed，0 ignored，0 filtered out，5.35s，exit0。父亲自读取成功日志，未将coder runtime完成单独当验收。

## Build identity and command

被测项目CLI SHA256 `00d5b19f85df2f2798abec7b1231181e1162a52a7fc9db170287a434597bc2aa`，包含已验证Released(None)状态修复；父通过shasum核对一致。新增测试曾按Cargo编译且执行1项，但同期运行时model接口在并行改动，后续Cargo重编译因此被阻断。父允许最终测试单独rustc编译到/tmp，并按首次成功Cargo fingerprint逐一固定八个依赖，不按mtime混选，未重建production或改共享产物。

```sh
PATH="$HOME/.cargo/bin:$PATH" rustc --edition=2024 --test \
  crates/codegen/shell/tests/test_released_delivery_finalization.rs \
  --crate-name test_released_delivery_finalization --cfg 'feature="default"' \
  -C panic=unwind -C codegen-units=128 -C debuginfo=line-tables-only \
  -C split-debuginfo=unpacked -C link-arg=-undefined -C link-arg=dynamic_lookup \
  -C force-unwind-tables=yes -C link-args=-ObjC -L dependency=target/debug/deps \
  --extern chat_state=target/debug/deps/libchat_state-773a5d6ec1eac61a.rlib \
  --extern tokio=target/debug/deps/libtokio-85ec2ff402e9f892.rlib \
  --extern serde_json=target/debug/deps/libserde_json-7d3b7a6b5c0dcead.rlib \
  --extern test_support=target/debug/deps/libtest_support-2a663b744f5a46f8.rlib \
  --extern tool_types=target/debug/deps/libtool_types-4f8bf8f6431a1a37.rlib \
  --extern uuid=target/debug/deps/libuuid-2e7d6c31e24d8861.rlib \
  --extern axum=target/debug/deps/libaxum-88913a633ec6645a.rlib \
  --extern futures=target/debug/deps/libfutures-da349408717c11cf.rlib \
  -o /tmp/grow-released-delivery-test-01a1a93a
RUST_MIN_STACK=16777216 GROW_BINARY="$PWD/target/debug/grow" \
  /tmp/grow-released-delivery-test-01a1a93a \
  released_delivery_accepts_real_terminal_result_and_completes_goal \
  --exact --ignored --nocapture --test-threads=1
```

源码rustfmt和补丁空白检查通过。此命令证据只归上述fixed binary，不作为新model-control实现验证。

## Failure diagnosis preserved

最初只对child第一次采样设置blocked expectation，父edit合法触发重采样后默认mock返回final，child未处理replan而以planning_completion_blocked失败。没有将Failed宽泛当取消成功：补诊断后确认父Ended/子Result都是该错误，夹具改为每个child foreground SSE都不发terminal，辅助请求独立正常应答，最终严格Cancelled断言保留并通过。没有修改production。

诊断期间一次直接将物理journal每行当TimelineEvent解析，missing at_ms，属于fixture解码错误；按真实Record{version,seq,checksum,fact}的domain事件提取后由Timeline::from_events做完整因果校验，无fallback/无中间事实写入。最终冷读不覆盖Shell storage loader的checksum/history/blob校验，这一点不影响真实CLI生成和canonical graph断言。

## Isolation

测试根为系统临时目录.tmpbMKoiu，其home/.grow和git workspace由TestSandbox创建/env_clear。parent/child provider分别127.0.0.1:57007和57009，未用真实凭据。测试断言自建writer被reap，额外只读进程检查同test binary agent stdio遗留0；provider任务Drop abort。没有安装、重启、控制正常会话或修改其存储，没有Git提交。隔离通过不能替当前正常会话的旧任务声明terminal或处理其replan。

归档前全量strict19/19通过；`openspec archive verify-released-delivery-goal-completion --yes --skip-specs`成功，无主规范变更。最后checklist在归档动作确认后完成，归档后全量strict18/18、archived850/850通过。源码修复与隔离完整链路证据闭合，正常旧会话状态仍独立未恢复。
