# PTY lifecycle ownership

## 依据与决定

准备记录见 `../complete-nondeferred-backlog/pty-lifecycle-preparation.md`。父已亲核 session.rs start/stop/status/wait_handle、pty.rs Child wrapper、server stop/WS 与 CLI run。portable-pty 0.9.0 的直接 child 句柄才提供 kill/wait 权限；输出 EOF 不是 child 退役，clone_killer 不代表可靠 kill+join。Session 应持有唯一结算 owner，不能让两段 waiter/stop 分别争抢同一个 child。

Session 不再由 reader 写 alive。alive=false 与 exit_code 来自 owner 的成功 try_wait/wait；观察/kill失败保留明确错误，不能声称终态。raw reader→feeder 沿 FIFO 把最后输出交给 terminal 后，才发布完整 terminal。stop、timeout 与自然退出汇合，重复 stop 复用同一结算，无重复 kill 或虚构退出码。I/O producer 必须有结束路径，不能因 Session 自留 sender 永远等待自身；不把同步 kill/wait/blocking PTY I/O 放进 async core 线程。

## 共享接口

新增必要的一个可克隆 `SessionControl`（名称沿实现约定可微调，但语义固定），从 `PtySession::control_handle(&self)` 取得，以便 build_router 消费 session 后 CLI 仍持有原 owner 控制权。接口：

- `stop(&self).await -> anyhow::Result<()>`：请求同一个 stop operation，成功仅表示直接 child 已真实结算且该 Session 的输出/feeder 退役；失败明确未确认，不丢 owner。调用方取消等待不能撤销已接纳的停止责任。
- `wait_for_exit(&self).await -> anyhow::Result<u32>`：观察完整 child+I/O 终态的真实退出码，无新控制副作用；closed/failed owner 返回 Err。自然输出 EOF 先于 child 退出不能返回成功。
- `wait_for_shutdown(&self).await -> anyhow::Result<()>`：服务策略观察；非 linger 的自然完整终态、显式 stop 或 timeout 的完整终态请求结束服务。linger 的自然终态仍保留服务，之后 stop 可使该观察完成。失败不伪装成成功退出。

PtySession 原 stop 可变入口同步改为调用此 handle；无后向兼容 wrapper要求。此三个操作共享现 watch/channel 风格，不建平行 Session registry。timeout 从成功 start 所属 owner 的创建点开始，不因 status/WS/重复 stop 延长；timeout=0 立即请求停止。局部 worker/信号/阻塞桥算法由 coder 选择，但必须有真实 owner 及失败语义依据。

reader EOF/错误、child exit 与 feeder完成分别是事实，不用一个 bool 代替。直接 child 正常退出后，若其它继承 fd 的进程阻止 EOF，完整终态不能无限卡住：给输出收束固定的有界 drain（默认一秒），保已读取 FIFO 并明确未确认 I/O 错误，不声称任意树回收。不得仅把 reader JoinHandle drop 当已经退役；若锁定平台 reader无法中断，先带证据上报父决定最小 handle 边界，不自行接受 leak。

## Session 平台实现依据

portable-pty 0.9.0 `lib.rs:114` 在 Unix 暴露 `MasterPty::as_raw_fd`；`unix.rs:352` reader 与 `:357` writer 都由 master dup 得到，关闭 master 不会中断其它 dup。Session 在 spawn child 前通过 master 的实际 fd 设置 `O_NONBLOCK`，reader/writer 都处理 `WouldBlock`、中断、部分写入和停止标志；不靠 close 某个 dup 或丢线程句柄宣称退役。

Windows `win/conpty.rs:95` 仅返回 opaque `Box<Read>`，但标准库 owned thread handle 可用于 `CancelSynchronousIo`。只对本次创建的 reader/writer 线程重复请求取消以覆盖 stop-check 与下一次同步调用的竞态；`ERROR_NOT_FOUND` 只表示当时没有 pending I/O，成功也不代表线程退出，仍以 join 为准。官方依据：[CancelSynchronousIo](https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-cancelsynchronousio)。

ConPTY 正常结束还需要关闭实际 master：`win/psuedocon.rs:73` 的 Drop 调用 `ClosePseudoConsole`。Windows 11 24H2 以前，该关闭可能等待客户端退出；官方要求关闭期间继续读 output，且不能由 reader 自己同步关闭。Session 的共享 master 使用 `Option`，在实际 child 已结算后由 owner 持有的独立 close worker 取走并释放，reader/FIFO 继续排空，close worker 也必须 join。close 未完成时不能为了 drain 期限主动撤掉 reader；close 完成后仍无 EOF 才对实际 reader 请求取消。已取走 master 的 resize 明确返回错误。官方依据：[ClosePseudoConsole remarks](https://learn.microsoft.com/en-us/windows/console/closepseudoconsole)。该资源关闭对 attached clients 的系统行为不构成按 registry PID 控制任意进程树的权限。

自主 std owner 线程先创建，再在其栈上创建实际 child 并持有所有 I/O worker 句柄；它只持外部 stop 状态的 Weak。start 接收端取消或最后外部 control/session 丢弃后仍进入同一停止路径，async runtime 退出不撤销责任。launcher 的 JoinHandle 不属于 public 完成承诺；public 完成证明实际 child 与全部 I/O worker 结算。owner 不因普通 I/O/channel 错误提前丢弃未 join 资源。terminal 使用 try_lock，锁竞争不阻止 child/stop/期限观察；generation 发送端保留到实际 child、I/O 与已读 FIFO 全部完成。

一秒 drain 或五秒 child stop 期限到期先返回明确 Err；原 owner 继续持有未确认的 child/worker 并请求停止、观察和实际 join。它不把错误后后台继续持有的责任报告成完整成功终态。Windows 正常自然退出通过 master-close 与并行 drain 具备成功结算路径；未经 Windows 实跑不声称平台验证通过。

### Review 修正：Windows child 观察

独立 review 核实 portable-pty 0.9.0 将 GetExitCodeProcess 的 259 一律当作 STILL_ACTIVE；真实退出 259 会使新 owner 永远等不到 drain。此处职责仍归 PtyChild：用本次 child 的 as_raw_handle 调用 WaitForSingleObject(handle, 0)，WAIT_TIMEOUT 表示运行中，WAIT_OBJECT_0 后再通过同一 child.wait 取得完整退出码，失败或缺 handle 返回错误。禁止 reopen PID 或引入第二 waiter；Unix 保留原路径。定向 Windows fixture 应输出末行后退出259并检查完整终态；本机只交 cross-check 证据，未实跑明确保留。该修正独立于 Service 接线，既有协议不变。

## Coder Task Specs

1. **Session 结算**：只改 ptyctl session/pty/wait必要消费和 crate内行为测试，以及本 change 的验证记录。Tier3，真实短命 child 保最后一行+真实非零code，EOF早于child不得假退出，显式stop/timeout确实结束实际spawn直接child，重复stop/取消wait保owner，linger自然退出不触发service shutdown但显式stop可触发。保原屏幕/resize/wait功能。先真实旧红。新接口及最终行为必须报告给父，再派server/CLI；不改server/CLI。
2. **Service 消费**：依上项已验接口，CLI在 move session 前保存control；bind/register/serve失败都结算本次child并保原错误/清理失败上下文。成功自然关闭服务为CLI成功，child code仍经status/WS交付。stop handler在外层锁内只克隆control，再释放锁await，失败返回明确HTTP错误。WS按同一个terminal handle发送closed(code)，慢发送/关闭有界；axum graceful也有固定有界收束，不让WS卡死进程。正常服务清注册只删仍指自己port的一代，--force仅覆盖名称记录，不停旧PID。真实CLI port0握手覆盖stop/timeout/natural/linger、打开WS终态、bind冲突和registry路径失败；隔离PTYCTL_SESSION_DIR/HOME/cwd，自有进程清理。

## Review Plan

父核单owner与状态/句柄寿命、同接口消费和输出终态顺序。Session先独立验收，service再打通真实CLI/HTTP/WS里程碑，不能把两套局部helper通过当产品链通过。数据/并发生命周期由独立reviewer审决定性diff和反例。必要main spec/docs/backlog和归档由父维护。Windows未执行明确保留限制，不扩大到Pager harness或任意进程树。
