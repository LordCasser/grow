## Evidence
native_image_read直接vec![0; len]；read_clipboard_image_from_class使用fs::read。pager-render只转发ImageData；wrap::fit_image_for_wrap在收到完整数据后才比较20MiB。当前native返回Option把不可用作为脚本回退信号，不能用None表达明确超限。

## Design
保留内部Option式AppKit探测，将closure结果设为Option<Result<ImageData>>并transpose，使native_image_read返回Result<Option<ImageData>>。get_image/get_attachments对Err立即传播；不可用仍回退。使用纯长度检查器统一50MB政策；文件reader通过take(limit+1)读取，不依赖可能变化的预先metadata。测试用较小注入预算和计数reader验证实际读取上限，生产调用固定50MB；测试长度边界不分配50MB。

## Scope
只约束Grow复制/读取的编码字节，不能阻止系统先构造NSData，也不声称是解压后像素内存限制。图片回退临时目录继续由既有RAII清理。未改传输预算或image解码器。
