## ADDED Requirements

### Requirement: External pager transcripts have private owned temporary files
普通 Markdown 和 minimal ANSI 分页器会话临时文件 SHALL 从创建起采用 Unix 0600 权限，并将清理责任绑定到待处理请求和分页器调用的生命周期。

#### Scenario: Transcript creation or replacement
- **WHEN** 创建会话文件或替换尚未打开的分页器请求
- **THEN** 新文件包含完整正文，部分写入失败清理新临时文件，被替换请求的旧文件释放并删除。

#### Scenario: Pager request ends
- **WHEN** 分页器正常结束、挂起返回非重试错误或应用正常释放待处理请求
- **THEN** 释放拥有的临时文件并尝试删除，无需依赖成功路径的显式删除。

#### Scenario: Suspend retry
- **WHEN** 挂起超时且请求等待重试
- **THEN** 同一文件所有权随请求保留，重试期间正文仍可读取。
