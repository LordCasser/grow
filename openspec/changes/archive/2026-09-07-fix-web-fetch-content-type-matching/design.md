# Design
HTTP 响应 Content-Type 原样进入 client，PDF 分支先于二进制拒绝，HTML 判断控制文本转换及输出类型。两个判断统一为分号前 trim 后 eq_ignore_ascii_case；HTML 只接受 text/html 和 application/xhtml+xml。参数不是分类依据。保留现有其余媒体检测，无新解析依赖。

通过类型矩阵以及 process_text_content 验证大写 HTML 真实转换与参数含 HTML 的纯文本原样保留；PDF 验证分支谓词。
