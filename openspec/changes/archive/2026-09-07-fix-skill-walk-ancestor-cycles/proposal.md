## Why
技能目录递归会跟随符号链接环，直到深度上限才停止。后续文件去重不能避免重复遍历与重复结果构造，需要在扫描当前祖先链上检测目录身份。

## What Changes
仅阻止返回当前祖先的目录链接，保留词典序、深度上限以及不同入口别名的现有行为。

## Capabilities
### Modified Capabilities
- configuration-rules: 技能扫描祖先环。

## Impact
tools 共享技能发现函数及调用它的配置、插件、注入来源。
