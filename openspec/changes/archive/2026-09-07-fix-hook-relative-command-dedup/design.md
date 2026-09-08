# Design
将原执行入口的 shell 字符判定搬到 config::command_uses_shell，runner 与 registry 共用，判定本身不增加语法。去重键增加 Option<PathBuf>：只有 HandlerType::Command、有效 command 为相对路径且不走 shell 时设置 source_dir。其他情况 None。

不 canonicalize，不依赖文件已存在，不跟随 symlink。现有 raw 键保留；同一目录相同相对命令仍合并，不同目录保留。shell、绝对命令跨目录仍合并。
