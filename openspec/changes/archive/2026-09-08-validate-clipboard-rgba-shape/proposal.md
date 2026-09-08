## Why
非macOS剪贴板RGBA编码仅检查buffer过短，过长数据传入image::PngEncoder后assert_eq panic。宽高乘法未检查溢出，arboard usize尺寸到u32强转可能截断。
## What Changes
编码前验证非零、尺寸可表示、乘法可表示及精确buffer长度，错误返回Result；纯编码函数移出平台模块供所有host单测，生产仍只在非macOS编译。
## Impact
arboard读取后的编码入口及纯函数测试，不改变有效RGBA输出，不限制arboard内部获取内存，也不新增跨平台抽象框架。
