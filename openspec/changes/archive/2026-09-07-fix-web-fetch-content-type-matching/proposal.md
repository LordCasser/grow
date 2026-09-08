# Why
HTML/PDF 使用大小写敏感的 contains 判断 Content-Type。合法的大写 HTML 不会转 Markdown，大写 PDF 被当作不支持的二进制；参数或非目标子类型包含目标字符串又可能触发错误转换或保存。

# What Changes
匹配分号前的媒体类型，忽略类型大小写并精确区分 HTML、XHTML、PDF。

# Impact
仅 web_fetch 类型分支判断、处理回归和说明，不调整其他格式策略。
