# Delta

## ADDED Requirements

### Requirement: Web fetch classifies HTML and PDF by media type
web_fetch SHALL 仅按 Content-Type 分号前的媒体类型判断 HTML/XHTML/PDF，忽略该部分大小写，参数和相似子类型 SHALL 不触发这些格式处理。

#### Scenario: 大写合法类型
- **WHEN** 响应为 TEXT/HTML、APPLICATION/XHTML+XML 或 APPLICATION/PDF
- **THEN** 分别按 HTML/XHTML 转换或 PDF 保存分支处理。

#### Scenario: 参数或相似类型
- **WHEN** 纯文本参数包含 text/html 或 application/pdf，或类型为 application/pdf-extra
- **THEN** 不因子串而进入 HTML/PDF 分支；纯文本内容保留，其他类型走既有分类。
