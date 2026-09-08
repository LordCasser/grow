## Context
`regenerate_completions` 在安装、链接更新和旧文件清理之后调用；逐个执行 bash/zsh/fish。当前无界 `cmd.output().await` 与补全不影响更新成功的意图不一致。
## Goals / Non-Goals
限制补全执行等待并清理直接子进程。不改变目录位置、下载流程、更新锁或任意派生进程树处理。
## Decisions
将单个 shell 的执行和写入提取为私有函数，生产循环与测试使用同一路径。使用 10 秒 timeout 和 kill_on_drop；只有成功且非空输出写入目标。测试全部使用临时目标，不读写用户真实 fish 补全。
## Risks / Trade-offs
极慢机器可能跳过一次补全刷新；已有文件仍可使用。一次更新三个 shell 最多分别等待 10 秒，不包含文件系统等待。Unix 用真实子进程检验，Windows 不运行 Unix 夹具。
