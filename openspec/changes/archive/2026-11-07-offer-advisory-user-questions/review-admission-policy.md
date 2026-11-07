# 独立准入审查

Reviewer：`01a1b071-7034-7b40-aeb9-3c5d28cdbccc`，只读，基于HEAD `8ab486a6e`与稳定未提交diff。覆盖Tools两模式/推荐验证、Shell Factory/worker/live配置和Pager Settings固定分支；卡片、迟答输入和真实PTY仍在实现，不属于此次结论。父亲读了决定性resolver源码，确认下项；待修复验证前不接受整个准入边界最终版本。

## P2：新增偏好类型/结构错误会默许开启建议

`util/config/resolve/toolset.rs::ask_user_question_allow_non_blocking_from_toml`用连续get/as_bool返回Option，无法区别未配置与present-invalid。live resolver加载generic TOML后调用BoolFlag，错误值变成None再默认true。例：`[toolset.ask_user_question] allow_non_blocking = "false"`，或toolset/ask_user_question存在却非table。文件语法合法不意味着策略字段合法；父核118–139实际路径确认，不声称reviewer已跑动态复现。

修复所有者为Shell新增live bool解析；不改其他timeout/search/login旧解析政策。只有缺失节点/键才允许默认true；无合法环境覆盖时，错误类型/结构返回Err，Tool::run在publication前失败。合法env覆盖仍高于合法TOML中的错误字段；文件本身read/语法解析失败沿当前Err语义，不能在此次修复悄悄放宽。无新公共error类型/配置来源框架。

通过标准：真实diskresolver缺省true、合法false、错误bool/string/int/array与错误中间节点Err；env true/false各自覆盖合法TOML的字段类型错误，移除env后恢复Err。实际Factory构建工具后把隔离配置改成此错误形式，后续真实ask调用在Shell reverse request publication之前失败，错误不能变为Agent推荐或用户答案。保留finalizedtimeout快照/raw字段和原17worker回归。先有旧红判别，再新绿；父核原review finding已消除后才接受。

## 其余审查结论和证据限度

已审稳定路径未发现其他可行动缺陷：effective mode入队前固定、等待不持资源锁；强制blocking允许缺推荐，显式非法推荐仍拒绝；advisory共用含排队的5秒绝对期限，仅Presented，无PendingInteraction；blocking错误Presented被durable解析为Failed；Factory保finalizedtimeout，Settings按顺序写raw并回滚，成功只确认saved偏好。

reviewer抽查Tools72ask/56output、Shell17worker/3policy、Factory1和runtime1项三场景green日志。runtime单结果计数来自provider history，未独立查Timeline事件数。Settings测试当时仍只有source；卡片/输入/PTY/Goal端到端未验收。该review不是全竞态无缺陷证明。
