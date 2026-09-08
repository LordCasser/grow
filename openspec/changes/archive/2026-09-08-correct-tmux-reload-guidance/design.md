## Evidence
[tmux官方手册](https://man.openbsd.org/tmux)说明配置在server启动时加载一次，后续通过source-file加载。普通attach不是配置加载操作。

## Design
通用与自动修复提示明确source-file作用于运行中服务器；truecolor说明保留reload后reattach与重启Grow的顺序，移除二选一关系。路径不能安全展示时要求手动加载变更文件，不再承诺重连即可生效。保留现有shell和Markdown引用处理。

## Verification
核对所有诊断、预览、后置输出及现有字符串快照的对应文字；全范围搜索排除旧替代提示。仅静态文字调整，不启动真实tmux或进行完整冷编译。
