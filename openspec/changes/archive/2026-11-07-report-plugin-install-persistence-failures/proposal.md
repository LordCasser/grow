# Report plugin install persistence failures

## Why

AC-08安装分支在文件已发布后吞InstallRegistry保存失败，CLI共享application和ACP actor都可能继续post-install并返回成功。已有registry坏读/坏JSON时，mutation还用容错load将错误当空记录，不能据此宣称完整安装状态已保存。

## What changes

只交付普通Git/local安装这项操作：共享`plugin::install_plugin`使用现fallible registry loader，在保存安装记录成功后才进入post-install/成功结果；保存失败返回既有typed错误，并明确文件可能已发布、记录未提交的部分完成事实。ACP Install复用同一application入口，不再复制安装/保存逻辑。CLI现public run的Err传播和失败diagnostic保留。

## Impact

不新建通用事务/回滚或wire状态；普通安装没有已验证的回滚，不许声称失败已撤销文件。卸载、更新保存及marketplace已有transactional owner分别处理。auto-enable已有warnings政策保持，本项只保证不能在核心registry保存失败后继续auto-enable。真实ACP与CLI公共dispatch、保存失败与坏registry控制、成功路径分别验证后才关闭AC-08安装子项。
