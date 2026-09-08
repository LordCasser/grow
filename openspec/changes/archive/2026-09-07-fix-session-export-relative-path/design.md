## Decision
直接用 agent.session.cwd.join(expanded) 解析路径，Path::join 对绝对输入保留绝对目标。无需新增路径配置或改变 Action 结构。

## Verification
真实 dispatcher 回归设置不同于进程 cwd 的会话临时目录，发送相对导出路径并检查文件实际位置、Markdown 内容。为使旧实现失败验证也不污染仓库，旧位置放入 tempdir_in 的受控目录。另验证绝对目标仍不被会话目录改写。

## Separate scope
同步直接写入的截断风险与文件原子替换另行处理。补全在相对 cwd 下的潜在重复拼接也需独立核对。
