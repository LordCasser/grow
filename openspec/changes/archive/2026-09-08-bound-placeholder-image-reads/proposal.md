## Why
占位图片加载用metadata检查后执行fs::read，文件增长时只在完整分配后拒绝，per_image_max不是实际读取预算。
## What Changes
使用文件流limit+1读取，超限沿用TooLarge；actual字段明确为观察到的字节数。保留前置授权、扩展名、文件类型、静态大小检查和后置MIME验证。
## Impact
client-support placeholder_images加载与恢复共用入口、错误描述和测试。不改变预算数值或路径授权规则；路径检查到打开之间的身份竞态另行审计。
