## Evidence
inline 使用 event_src.rfind(url_str)，reference 使用 def_src.rfind(url_str)。pulldown_cmark 返回的 URL 已经语法解析，不能仅凭最后一次字符串出现推断源码中的目标位置。

## Constraints
保留 Markdown 标签、title、代码片段和 reference 标签，不能简单把 rfind 改成 find（链接标签本身也可能与目标同名）。继续只处理技能目录内存在的路径；路径越界检查不得放宽。转义、空白及引用定义需要与 parser 的源码范围核对后选择实现。

## Status
本轮先建立真实同名标题回归；生产修复尚未实施。

## Selected implementation
inline 对已识别的链接源码再解析，内部标签事件的最大结束 offset 给出标签内容边界（排除覆盖整个链接的 wrapper 范围）；从其后的 ]( 读取目标。reference 从未转义的 ]: 读取目标。小型目标扫描只处理反斜杠转义、尖括号及平衡括号，Markdown 的链接有效性仍由 pulldown-cmark 判定。替换时保留 raw label/title，转义目的地址中的 Markdown 特殊字符，裸目标遇空白改用尖括号。不重写整篇 Markdown。
