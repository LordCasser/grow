# Parent review

生产修复落在正确的Shell bootstrap owner，复用同一session_plugin_registry，不改变LSP enablement、sourced loader/trust/precedence、继承gate或生命周期。旧实现真实ACP反例和最终三阶段进程记录已亲核；以下F1已由独立fixture修订实际通过，父接受本项功能及实际bootstrap进程里程碑。

## F1：same-cwd记录聚合不能单独证明每个session的来源

原phase1先创建A和B，再按共同rootUri聚合labels和计数。若两个session对调插件命令，最终集合仍是session-a/session-b/user，断言可能通过；rootUri相同使它无法区分各session。这不是已确认的生产回归，但不足以证明本次契约中的各自session snapshot。

同一fixture最小修订：创建A后先等待其last-key user握手，核仅session-a/user且各一次、没有session-b；保持A存活再创建B，等待第二次user握手，核新增session-b一次且session-a没有重复。其余矩阵、production、旧红均保持。以同一最终target一次实际通过验收，不增加API或框架，不重复无关模块回归。

进程验证限制：现辅助谓词把gone或Z视为不再运行，不据此笼统宣称每个PID都已wait/reap；最终父另核本轮记录PID已不存在。launch workspace backend的fixture清理与session ProcessScope各自归属保持，不能混报为session leak或本修复带来的生产回收承诺。
