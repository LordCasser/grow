# Service consumer verification

## 实现与边界

Service consumer 已实现，生产文件为 `ptyctl/src/server.rs`、`ptyctl-cli/src/commands/run.rs` 和 `ptyctl-cli/src/registry.rs`；CLI manifest 仅给既有 reqwest 增加 `query` feature。未修改版本、root features、lock、Session/PTY/wait、Shell/Pager、主规范、docs 或 backlog。旧 Service 基线保存在 `/tmp/ptyctl-service-01a1b479-baseline/`。

CLI 在 Session move 前保存原 control，bind/local-address/register/serve 失败都停止本次 owner，并保留 primary 与 cleanup error chain。正常自然服务退出返回 CLI 成功，child code 通过 status/WS 交付。HTTP stop 在 outer Session lock 内仅克隆 control，锁外等待，结算失败返回 500。WS 从同一 control 的完整终态取 code，接受 upgrade 前订阅输出，终态后先排该订阅已接纳 FIFO 再发送 closed；单次发送/close 500ms、终态 flush 1s。Service 跟踪已接纳的升级连接，control shutdown 后 graceful drain 最多 2s，超时明确返回 Err。

registry 使用标准库 `File::lock`；register 的 temp-write/rename 和 cleanup 的 read/compare/delete 共用常驻 `.registry.lock`。每个事务独立 open，锁文件不删除；比较 port、started_at、PID，保留已替换的一代。`--force` 仅替换登记，不停止旧 PID。原 atomic rename、无锁 lookup 和其它 registry 安全边界保持。

## 环境与执行

命令在同一独占 Cargo 窗口串行运行，使用 `PATH="$HOME/.cargo/bin:$PATH"`、`CARGO_BUILD_JOBS=2`、`CARGO_INCREMENTAL=0`、`RUST_MIN_STACK=16777216`、Rust 1.93.1。所有编译在后台执行且 `timeout=0`，均带 `--locked --offline`。

先前父任务 `01a1b485-c186-7b51-8ffa-0fa73a67eb9a` 的 208s exit101 是 reqwest 0.13.4 `query` feature 缺失造成的产品 CLI 编译失败，不计行为旧红。本窗口仅补 CLI feature。首次重跑 `/tmp/ptyctl-service-oldred.log`（task `01a1b4a6-18e0-7711-91e6-602eb74df9a6`）在 fixture 的 reqwest client 构造时缺 TLS provider，同样不计；fixture 后复用真实 CLI 已有的 `diagnostics::tls::install_ring_provider_once()`。

核心旧红、新绿执行同一命令：

```sh
cargo +1.93.1 test -p ptyctl-cli --test service_lifecycle \
  natural_exit_closes_cli_service --locked --offline -- --exact --nocapture
```

| 检查 | 实际结果与原始日志 |
| --- | --- |
| 有效旧红，task `01a1b4a6-e812-7b83-ba2e-2a7e76ed42ea` | exit101，编译 2.60s；quiet port 64371 握手成功，HTTP `alive=false`、`exit_code=23` 后 CLI 超过三秒仍未退出，3.43s 触发预期断言。`/tmp/ptyctl-service-oldred-confirmed.log`。 |
| 核心新绿，task `01a1b4ab-1398-7222-8231-fced6738972a` | exit0，1/1，通过同一 code23 终态与正常 CLI 退出检查，测试 0.64s。`/tmp/ptyctl-service-newgreen.log`。 |
| 最终真实 CLI 集成，task `01a1b4b0-53ff-7422-8fb9-b725d00df8d8` | exit0，10/10，测试 5.78s。`/tmp/ptyctl-service-integration-verified.log`。 |
| ptyctl library，task `01a1b4b0-f5b1-7392-8fa1-94c6336a2c1c` | exit0，30/30（原29加 HTTP settlement error1），测试 9.38s。`/tmp/ptyctl-service-lib.log`。 |
| registry private，task `01a1b4b1-5b80-7600-a1d5-33fbfc6e61b8` | exit0，2/2，测试 0.08s；同 port/PID 新代不误删、writer 不能绕过整个 compare-delete transaction。`/tmp/ptyctl-service-registry.log`。 |
| Windows library/tests cross-check，task `01a1b4b1-b0eb-76c2-8708-5114b6582d4d` | exit0，118.10s。`/tmp/ptyctl-service-windows-check.log`；包含 Windows lib 与 test 类型检查，非 Windows 实跑。Windows observer 文件现有 unused import warning 未在本项越界修改。 |

实际执行命令：

```sh
cargo +1.93.1 test -p ptyctl-cli --test service_lifecycle --locked --offline -- --nocapture
cargo +1.93.1 test -p ptyctl --lib --locked --offline -- --test-threads=1
cargo +1.93.1 test -p ptyctl-cli --bin ptyctl registry::tests --locked --offline -- --nocapture
cargo +1.93.1 check -p ptyctl --lib --tests --target x86_64-pc-windows-msvc --locked --offline
```

## 集成覆盖与夹具修正

`ptyctl-cli/tests/service_lifecycle.rs` 共10个 Unix 场景，真实 `CARGO_BIN_EXE_ptyctl`，各自隔离 `PTYCTL_SESSION_DIR`、HOME、cwd，以 port0 quiet 握手。覆盖 stop/timeout 实际 child 已退出、自然退出、WS 最后输出在真实 code 前、linger 最后 screen/status、stop 加 open WS、慢连接有界、bind/registryfile 失败清理和 force 替名。只清理自己创建的 CLI；启动失败 child 自限，不通过 registry PID 杀未知进程。WS 使用合法16字节 key，握手最多3s。

首轮10场景为9绿1失败，原始日志 `/tmp/ptyctl-service-integration.log`。失败场景原先用8MB输出，在五秒 CLI 检查及进一步30秒实际 child 观察中仍未完成输出，未到 Service shutdown 起算条件（`/tmp/ptyctl-service-slow-connection.log`）。同一场景改为1KiB TCP receive buffer与256KiB输出，保持未读 WS 和不完整 HTTP request；没有新增场景。之后真实 child 可退出，CLI 明确返回 `HTTP/WS service did not drain within 2 seconds`（`/tmp/ptyctl-service-slow-connection-final.log`、`/tmp/ptyctl-service-slow-diagnostic.log`），原成功断言与“明确Err不能冒成功”要求冲突，已改为精确验证此错误、有界退出及实际 PID 已退出，父已确认该语义。

另一次套件9绿1失败是 status poll 与 listener 正确关闭的竞态（`/tmp/ptyctl-service-integration-final.log`）；poll 现容许 CLI 已退役或连接已关闭，最终10全绿。以上修正只影响夹具，生产源码在核心新绿后保持冻结；保留全部失败日志，不把编译/启动/夹具错误算作行为旧红。

## 自查、review 与限制

`git diff --check` 已通过。父任务消息确认独立 reviewer 接受当前冻结生产源码；本执行者未把自查称为独立 review。生产 SHA256：

- server.rs：`59b72d2e7bee9b3c28b4540e9bba0fa60ef8815df7ac7a8f78968149822188c5`
- run.rs：`71e636cc7c0e9fcefb32152e5c759daa63eafe78c77956fb1dbba20edcbcbbb5`
- registry.rs：`92043b79bdf7b909651e4051959d3a0165b25b42d971d1ec8285b32b554d7395`

所有已授权 Cargo 命令已结束，Windows cross-check exit0 后已明确向父任务释放 Cargo 窗口；本执行者不再运行 Cargo 或修改生产源码。最终夹具仅执行一次机械 rustfmt，生产 hashes 未变，未重复已绿套件。

Windows 实机未运行；cross-check 只覆盖 Windows 编译与 source-ready exit259 observer/test 的类型检查。主规范同步、strict/archive 及总体发布门禁由父负责，本项不执行归档或提交。
