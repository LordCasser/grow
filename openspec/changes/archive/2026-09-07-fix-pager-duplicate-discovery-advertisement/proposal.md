## Why
Pager 的 Skills discovery 事件在补挂目录后同时发送 ReloadSkills 与 AdvertiseCommands。前者会在重读完成后自行发布命令，后者导致额外 workflow 扫描及过早发布旧技能快照的机会。

## What Changes
Skills 事件只请求重读；由既有重读完成路径负责发布。Workflows 事件保留直接发布，目录补挂仍先执行。

## Capabilities
### Modified Capabilities
- client-surfaces: Pager discovery 事件的发布职责。

## Impact
仅 Pager ACP watcher 消费分支；不增加去重状态、不改变扫描实现、不处理多个独立 watcher 之间的合并。
