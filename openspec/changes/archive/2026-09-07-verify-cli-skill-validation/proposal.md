## Why
工作区技能发现、损坏元数据校验及引用索引修复已完成局部回归，但 CLI 仍为此前链接产物。需要重新构建，将已验证源码汇总成实际可运行程序并记录准确产物身份。

## What Changes
低磁盘构建 CLI，执行早期命令烟测，记录文件哈希及磁盘。

## Capabilities
纯构建验证，skip_specs=true。

## Impact
更新 target/debug/grow，不替换已安装版本。
