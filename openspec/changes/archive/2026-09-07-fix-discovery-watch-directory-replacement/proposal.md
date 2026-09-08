## Why
ProjectDiscoveryWatcher 已有.grow时只监听.grow本身；删除后无父目录监听接住重建。attach_new_refresh_dirs 只按曾注册路径去重，不区分替换实体。SkillsFileWatcher 共用该辅助逻辑。

## What Changes
保留项目父目录观察能力；恢复 seed 目录监听时识别替换，不仅检查路径曾存在。

## Capabilities
### Modified Capabilities
- configuration-rules: discovery 目录替换后的监听恢复。

## Impact
ProjectDiscoveryWatcher/SkillsFileWatcher 注册维护；不得在每次文件修改时重扫整个递归技能树。
