## Reproduction
旧实现 save_preserves_unreadable_config_bytes 失败：真实临时文件包含非法 UTF-8，save_config_at 返回成功而非错误。该函数成功路径写入新 TOML 并 rename 覆盖原文件。

## Validation
shell util::config::persist：55 passed，包含非法 UTF-8 字节保留、TOML 语法错误拒绝保存与缺失文件创建。macOS linker 既有 __eh_frame 大小警告不影响测试结果。

## Scope
不设置 GROW_HOME、不读取或写入真实用户配置。update_config 的初始失败传播通过源码核对，其 ? 位于修改闭包调用之前；本次不以全局环境变量测试该公开入口。reset 来源所有权问题单独记录 backlog，未改行为。
