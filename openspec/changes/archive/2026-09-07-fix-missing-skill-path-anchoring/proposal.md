## Why
技能添加承诺保存绝对路径，但请求 cwd 为默认点目录且目标不存在时，canonicalize 失败会返回相对路径。配置含义因未来启动目录而变化，需独立于文件存在性锚定路径。

## What Changes
在 canonicalize 前使用标准库 absolute 锚定请求拼接路径，不依赖目标存在。

## Capabilities
### Modified Capabilities
- configuration-rules: 不存在技能路径的锚定。

## Impact
shell 技能路径解析及回归；不改变已删除链接的历史目标解析。
