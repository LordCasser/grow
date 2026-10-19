# Evidence and decisions

`agent/src/builder.rs` 已在构建期调用 list_skills_with_plugins；SkillManager seed/reload 统一管理 native/plugin/条件目录，load_skill(name,args) 已执行严格准备和发布。保留此架构，不另建永久索引。扫描只保留已有有界 frontmatter/摘要读取，正文在激活后按需读取。

当前 resolve_skill 先排除 disabled 再计算裸名，可能让同名启用来源接管名称。改为在目录上计算稳定名称，再检查资格，未知/歧义/禁用/用户或模型不可调用分别明确拒绝。只接受目录 token，不把路径当技能名称。被禁用名称仍不作为可用技能展示；被请求时返回具体失败和禁止绕过的提示。使用策略在工具定义和目录头统一描述，普通 Read 文档明确只作文件审阅，不能代替加载。

官方 [best practices](https://agentskills.io/skill-creation/best-practices) 和 [specification](https://agentskills.io/specification) 建议元数据、主说明、按需资源三层，主说明尽量精简。Grow 的实现不是自动分片或检索全库：`load_skill(name)` 返回主说明；`load_skill(name, resource="references/api.md")` 仅返回该文件。主说明中的条件由模型判断；不预读同级资源。现有相对链接解析仍保留，加载结果保留实际 source，工具定义与目录头说明资源参数的使用方法；绝对链接不成为绕过加载的指令。

资源路径须为技能根内普通相对路径，拒绝绝对路径、父级跳转、Windows 驱动/UNC/反斜杠别名及解析后逃逸根目录的符号链接。资源读取通过配置 FileSystem，沿用 1 MiB+探测字节、严格 UTF-8、取消、gitignore；内容按原文返回，不把引用文档中的 frontmatter 或 $ARGUMENTS 当模板。资源来源在 preflight 解析，授权针对实际文件；执行前重新验证相同路径，避免批准主说明后读取另一路径。URI/frozen-only 技能没有可访问资源根时明确失败。读取不执行脚本也不递归加载其他资源。

主说明与资源结果都复用现有 SkillLoadModel 提交，资源正文内带名称/相对资源身份，raw output 保留实际 source；因此无需增加 Timeline schema，也保持 commit ACK、compaction 保护和 resume 的原始内容快照。资源不接受 args，避免混淆模板调用与资料读取。仅声明初始化预加载保持独立机制；失败返回明确配置错误，不装作具备技能继续执行角色。

# Interaction and limits

- 禁用检查每次调用执行前复核；已提交的历史正文保留历史事实，不伪造删除或追溯取消。
- 条件技能仍按现有 paths 激活；目录冲突必须明确限定来源。权限/Hook拒绝不授权自动换工具读取。
- 不修改用户 ~/.grow 角色文件或设置。旧角色中的路径指示由更明确的运行时加载政策约束。
- 不把该工具包装为 OS 访问隔离；普通文件审阅保留，无法通过静态测试证明任意模型都遵守指令，增加真实请求/结果管线断言并记录行为评估边界。

# Verification

名称冲突与禁用、不可调用、预加载失败；不携带全部正文的目录；默认只读主说明；resource 按需单文件；大小/UTF-8/越界/链接逃逸/取消；模型工具管线的 resource 持久化和明确失败。现有 Skill ACK 丢失/提交失败/取消/冷恢复测试继续运行，确认未削弱跨层时序。

# Additional consistency finding

`grow_build/read_file` 还保留了旧技能加载路径的例外：遇到 SKILL.md 或 skills 目录下 Markdown 就忽略 offset/limit 和 token 上限。现在完整指导由 load_skill 保证，保留该例外会让普通编辑读取无意吞入完整说明。因此移除该例外，三个 Read 变体共享普通行窗口；不拦截合法文件审阅、不伪造技能激活。此项属于本次加载职责分离，不扩大为通用文件沙箱。

# Cross-layer validation finding

模型管线回归暴露 Timeline 的 ModelLoadSkillArguments 仍只允许 name/args，工具新增 resource 后被持久化校验拒绝。同步扩展这个独立的事实校验器，同时比较当前与已接纳调用的完整结构化参数（包括 resource），不放开未知字段，也不引入工具层依赖或新的 Timeline 字段。资源身份继续由原始调用和提交正文保存。
