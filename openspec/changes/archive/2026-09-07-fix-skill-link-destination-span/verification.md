## 旧实现失败
完整 Cargo tools 定向 internal_link_title_matching_destination_stays_literal 失败。实际结果：[guide](guide.md "<临时目录>/guide.md")；期望目标变绝对路径、title 保持 guide.md。真实临时文件存在且在 skill_dir 内。测试包含 reference 情形，但循环在 inline 断言失败，不能声称 reference 也已独立复现。

生产实现尚未修改，新增回归当前会失败。下一步必须选择可靠目的地址源码范围方案，不以 find/rfind 互换掩盖标签或转义边界。

构建使用关闭 incremental/debug 的统一环境，命令 cargo test --locked --offline -p tools --lib internal_link_title_matching_destination_stays_literal --quiet。

## 已完成修复
基于 parser 内部标签事件范围定位 inline 标签边界，从 ]( 后读取目标；reference 从未转义 ]: 后读取。扫描处理尖括号、转义和平衡括号，替换只作用于目的地址源码范围。输出处理 &、反斜杠和尖括号转义；含空格路径使用尖括号。没有放宽技能目录 canonical containment 检查。

完整 Cargo tools lib internal_link 4 项通过（每项含多个场景），再运行 implementations::skills::skill::tests 全部 53 项通过。inline 与 reference 标题同名均验证成功；额外标签同名、代码、空标签、嵌套图片、引用定义，以及真实转义/实体/空格路径通过。特殊路径输出重新交给 pulldown-cmark 检查实际 dest_url，而非只检查字符串中出现路径。

未运行 UI 端到端测试；URI fragment/query 的文件定位规则未在本次扩展。使用统一关闭 incremental/debug 的 Cargo 环境。
