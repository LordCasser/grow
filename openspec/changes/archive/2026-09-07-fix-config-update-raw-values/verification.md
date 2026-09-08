## Reproduction
旧 update_config_at 使用实际 config::load_config_file；只修改 disabled 后，回归失败：paths 中 ${HOME}/skills 被替换为本机 HOME 绝对路径。

## Validation
shell util::config::persist：57 passed。包含原始引用保留、disabled 正常修改，以及非法 UTF-8/语法错误时闭包不执行、原字节不变。macOS linker 既有 __eh_frame 警告未影响结果。

## Limits
使用临时配置文件及已有 HOME，无全局环境修改。版本覆盖不写回由新路径只调用原始 TOML 解析的源码核对支持，未操作全局版本注册器进行集成测试。显式 save_config(Config) 仍保存调用者提供的值，不保证其来源未经展开。
