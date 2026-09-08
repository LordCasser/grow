## Design
保持源读取的4 MiB与NUL规则。最终渲染后、构造inspection和plan前按字节长度拒绝超限，以最终总内容覆盖原文、标记和所有条目。NUL与已有正文CR检查并列，返回InvalidRequest。超限输出返回含目标路径的UnsafePath，说明planned config exceeds。

## Verification
恰好4 MiB的输出可应用且再次规划为NoChange；超过1字节应在plan拒绝。已有4 MiB配置追加短条目应拒绝并保留源，不创建备份或临时文件。含NUL条目无论源是否存在都拒绝且不写入。

## Limits
渲染前请求大小和总内存预算不在本次承诺内；本次保证不发布已知无法读取的输出。

注释前缀也是渲染输入；CommentSyntax::new 的现有单行校验同时拒绝NUL，避免通过前缀绕过正文限制。
