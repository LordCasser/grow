## Why
技能缺少描述时，发现器 read_to_string 读取完整文件，再取 2 KiB 正文生成描述。大正文会导致无必要的读取与分配，应将元数据预览预算放到底层读取。

## What Changes
描述回退最多读取 frontmatter 预算加 body peek 预算及一个截断探测字节；显式正文加载不变。

## Capabilities
### Modified Capabilities
- configuration-rules: 描述预览读取预算。

## Impact
tools 技能发现描述回退，超出预览预算的内容不参与描述。
