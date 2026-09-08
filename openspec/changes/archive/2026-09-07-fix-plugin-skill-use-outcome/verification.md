## Evidence
- 修改前 admission 在调用展开函数之前构造 PluginUsed { success: true }，不读取任何加载结果。
- 现在共享展开函数为每个输入引用保留一个 loaded 结果，成功生成块后才置 true。admission 按同一输入顺序 zip 后设置 PluginUsed.success，同时在 plugin.used span 记录 success。
- 临时文件回归覆盖单项成功、缺失文件、混合 [true, false, false]（成功/文件缺失/目录项消失）、空输入；正文及索引断言继续保留。
- 完整 session::slash_commands::tests：97 passed，0 failed。命令使用 locked/offline、无增量、debug=0、2 jobs、RUST_MIN_STACK=16777216。
- interjection 调用点只读取 information，原有派发事件未变；这是源码核对，未新增诊断 sink 集成测试。
- 编译仅有既有 macOS __eh_frame compact unwind 警告。

## Limits
成功表示正文块生成，不是模型任务完成。没有重新链接 CLI 或替换用户已安装 grow；此次已编译并验证 shell 测试目标。
