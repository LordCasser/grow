## Before
回归 build_skill_information_preserves_selected_identity_at_shared_path 在旧查找逻辑失败：请求 /inspector:review，生成块名称仍为 inspector:review，但正文实际为 Native snapshot。测试使用有效的预加载正文快照，未访问用户文件。

## Change
目录查找从仅比较 path 改为 path + plugin_name + format_skill_name 三项相等。ParsedSkillRef 的既有字段足够，不新增身份实体，也不重复读取正文。

## Coverage
回归验证插件正文、GROW_PLUGIN_ROOT 替换、排除原生正文，以及插件目录项消失后 loaded=false、information=None。共享函数同时服务 admission 与 interjection。

## Limits
不是模型执行端到端测试；本次不重新链接 CLI 或修改用户安装目录。完整 session::slash_commands::tests：98 passed，0 failed；locked/offline、无增量、debug=0、2 jobs、RUST_MIN_STACK=16777216。编译仅有既有 macOS compact unwind 链接警告。
