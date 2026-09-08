## Evidence
arboard_get_image在调用encode_rgba_to_png前用as u32转换尺寸。encoder函数使用usize直接乘法，仅检查rgba.len()<expected。已安装image0.25.10 PngEncoder::write_image在PNG编码前assert_eq!(expected_buffer_len, buf.len())，输入多1字节足以触发panic。

## Design
将既有纯函数移到clipboard父模块，cfg(any(test, not(macos)))；测试改为全平台运行。先用原实现复现过长buffer与溢出，再让入口接收usize以便检查arboard原尺寸，使用u32::try_from及checked_mul。要求buffer长度恰好相等，拒绝零尺寸。保持现有短buffer错误可读性和成功路径PNG编码。资源总预算不混入该形状校验。
