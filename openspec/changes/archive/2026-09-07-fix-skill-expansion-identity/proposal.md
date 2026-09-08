## Why
命令选择保留限定名称和插件身份，但展开只按路径取首项。原生与插件技能可共用文件，路径相同不代表正文快照及插件变量相同。

## What Changes
展开目录查找同时核对路径、限定名称及插件身份，防止同路径其他来源替代已选技能。

## Capabilities
### Modified Capabilities
- configuration-rules: 技能展开保留选择身份。

## Impact
shell 共享技能展开查找，覆盖 admission 和 interjection。不改变发现与命令选择规则。
