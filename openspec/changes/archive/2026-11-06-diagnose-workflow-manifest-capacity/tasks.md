# Tasks: diagnose-workflow-manifest-capacity

- [x] 定位失败错误来源（store.rs 524288 整包 JSON 检查点）
- [x] 核实 manifest 范围：`WorkflowRunManifest` 仅 {version, state, script_revision}；script/args 走独立文件不进 manifest
- [x] 核实 tracker capture → skill_catalog（含 body）序列化路径
- [x] 修正：`capture_agent_definitions`（definition.rs:89-124）含 builtin + user/project discovery，`for_test` 空 `cli_agents` 不等于 definitions 为空
- [x] 核实 `for_test`/`SkillsConfig::default` 的 discovery 默认 roots（`~/.grow`）
- [x] 静态统计本机真实技能字节量级（269,403 bytes / 21 skills；仅候选输入量级，非归因证据）
- [x] 排除其他当前输入源（GROW_ROOT/GROW_USER/GROW_HOME 为空；bundled/plugins 不存在）
- [x] 区分 production 现有硬上限（非已确认 bug，不提调限/压缩实施）与测试非 hermetic 事实（越界成因未证明），写入 analysis.md
- [x] Control A：固定已编译测试产物hash，仅隔离测试子进程HOME/GROW_HOME；147 passed / 0 failed、exit0，证明25项失败依赖发现环境，不修改父环境或真实用户文件。
- [x] 依据control收口：不提高产品上限、不引入压缩/分片；字段精确字节和Control B不再作为本次归因门禁，若以后真实配置触顶再独立验证。
- [x] 记录本机发现输入对测试的影响与隔离验证证据，将其作为独立工具维护债务，不混入F4行为修复。
