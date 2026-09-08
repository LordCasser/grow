## ADDED Requirements

### Requirement: Skill description previews bound underlying reads
描述回退 SHALL 在底层读取时限制为 frontmatter 预算加正文预览预算，允许额外一个探测字节；不得为生成短描述完整读取任意长度正文。显式技能正文加载不受此预览上限影响。

#### Scenario: Large body needs fallback description
- **WHEN** 技能需要回退描述且文件超过预览预算
- **THEN** 只读取预算及一个探测字节，在预览内生成描述。

#### Scenario: UTF-8 crosses preview boundary
- **WHEN** 截断边界落在多字节字符内部
- **THEN** 丢弃末尾不完整字符；预览内部非法 UTF-8 仍报读取错误并回退技能名。
