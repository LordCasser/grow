# Design

Workspace permission 已拥有解析后词序列的 rg 外部程序例外。扩展该现有判定覆盖精确 `--hostname-bin` 与 `--hostname-bin=...`，并让 manager 的 safe/always-safe、evaluate_bash 的非精确 grant 防线、default scope 和 Auto routine 共享相同政策。原 `--pre` 同时保留，`--pre-glob` 自身仍是过滤参数。沿原 `--pre` 保守策略扫描所有参数；不另建 rg 选项解析器。带有执行选项形状的字面模式也可能进入普通授权复核，不能因无法证明的 `--` 提前停止扫描而放过外部程序。不新增权限等级或另一份命令 registry。

敏感效果是已被视为只读搜索的命令启动用户选择的可执行程序。不变量：内置观察命令或一个普通 `rg` 前缀 grant 不能提供该任意程序的执行权限；有执行/写入上限的子 Agent 也不能借此得到更高能力。精确 invocation grant 和 always-approve 的既有用户策略不属于漏洞证据，仍需遵循显式 deny。

已核路径：`permission/manager.rs::is_safe_command_words`、`is_always_safe_command_words`、`evaluate_bash`、`default_always_allow_scope`；`auto_mode.rs::is_routine_segment` 的 rg 分支；Shell `shell_required_access` 调用共享 `command_is_known_observational` 后选 RX/All。因此优先修共享 owner，Shell 只补真实投影/能力天花板回归，若现调用合同已足够则不改其生产实现。

父实验 `/tmp/grow-rg-hostname-probe.py` / `.log`：在自有 TemporaryDirectory 写 helper，只往同目录 marker 追加固定字符串并输出固定hostname；普通搜索不启动helper，两个hostname参数形式和关闭hyperlink场景均启动，参数分隔符后的字面模式对照不启动。执行5秒预算，无真实用户配置/网络/数据写入。后者最初额外operand导致exit2，只能证明未触发，不能算成功搜索正控；产品测试应使用合法字面pattern/file替代。

交接：coder在 Workspace permission 一个owner修这项分类及关联测试；先验证现代码真实Ask/Auto/前缀grant结果，再修，实现范围可含Shell投影测试但不得扩邻接模块功能。优先复用既有 rg --pre 测试；若选项终止符/值操作数与现parser策略不同，按真实解析与实验说明，不能凭字符串包含推断执行。

父 review 实验 `/tmp/grow-rg-hostname-probe-review.log` 又证实：`--glob -- --hostname-bin HELPER needle FILE` 的第一个 `--` 被 glob 消费，仍会启动 helper（exit0）；候选实现直接遇 `--` 返回 false，形成同一权限缺陷的新绕过。撤回该新增优化，保留旧 pre 检查的保守全参扫描。`--hostname-b` 在本机15.2.0被拒且不执行，不作为缺陷。`--regexp --` 用例也启动但exit2，只用于证明初始化效果，成功搜索对照以glob用例为准。

验收：普通不含执行选项形状的搜索和 `--pre-glob` 保持；空格/等号hostname参数拒绝safe/routine/RX；Ask进入实际prompt，Auto不走routine静默允许，普通rg prefix不足而exactgrant保持；受限子Agent实际freeze-call投影不再为RX；deny优先控制。加入glob参数值消耗 `--` 的真实入口回归；字面执行选项可能额外复核作为保守边界明确记录。先记录old-red再green，副作用仅临时marker。Cargo由父单独授窗口，Rust1.93.1 locked/offline。
