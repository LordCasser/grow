## Design
提取 transcript_pager_command 返回 io::Result<Command>，先 shlex::split 并验证首参数，再添加既有 less -R/+G 和独立文件参数。闭包调用 command.status，错误继续走刚归档的恢复后反馈。默认 less 的环境选择不变。

## Validation
参数表覆盖引号、转义空格、空参数、非法输入、less 参数和非 less；临时目录内带空格脚本捕获实际 argv，验证文件路径及 shell 特殊文本按字面传递。不执行用户环境配置。
