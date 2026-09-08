# Design
command key 由 command_raw 转 OsString，否则 command.as_os_str().to_owned，均缺失才默认空。URL key 由 url_raw.or(url)。raw 用于原文优先是现有契约；不为了显示字段缺失把所有条目当同一内容。

回归分别覆盖 command 与 HTTP，raw None 时不同实际值保留，相同实际值仍合并且高优先先出现者胜出。source_dir 引起相对命令执行身份差异另记审计事项，不混入本修复。
