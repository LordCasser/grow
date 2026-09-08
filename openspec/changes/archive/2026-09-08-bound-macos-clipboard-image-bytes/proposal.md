## Why
原生剪贴板图片读取根据NSData.length无上限分配Vec；脚本回退fs::read也无上限。wrap传输20MiB和placeholder50MB限制处于不同或更晚的路径，不能保护这些读取。
## What Changes
macOS编码图片读取统一限制50,000,000字节。原生数据超限在Rust复制前报错，不转为None重试脚本；回退文件用limit+1流读取拒绝超限。保持空图片、格式优先和临时目录生命周期。
## Impact
client-support剪贴板原生与回退读取，调用方错误传播。50MB是新的本地读取策略，允许大截图且限制重复内存占用；不复用placeholder常量，避免耦合不同入口政策。OS在dataForType内部的分配、解码预算、非macOS读取另立项。
