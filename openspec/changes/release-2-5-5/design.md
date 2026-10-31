## Approach

保留当前实现及已归档行为，按主题拆分 Git index；共享规范和开发说明按对应段落提交。warning 以项目固定工具链实际诊断为依据，检查默认生产图和包含测试的完整工作区，避免用测试依赖掩盖生产编译缺口。不得用全局 allow 或升级工具链隐藏问题。

版本从 2.5.4 升至 2.5.5。annotated tag 绑定完整源提交，使用官方 LordCasser/grow release.yml 的全部平台与 publish=true；现有 Core、native、Windows、OpenSpec 与构建门禁决定是否发布。记录失败及处置，不跳过失败测试。发布完成后核对版本、源提交、十个归档和 SHA256SUMS，再清理本次编译残留。

## 实际诊断与修复

jemalloc-sys 的 NO_UNPREFIXED_MALLOC_TARGETS 明确列出 apple/android/dragonfly；当前 workspace 无条件请求 unprefixed_malloc_on_supported_platforms，使 macOS 每次输出 build-script warning 后回退 prefixed。将该 feature 移至 CLI 的对应目标依赖，profiling/stats 和全局 allocator 入口不变；不通过 override_allocator 改变 Apple 的 malloc zone 行为。受支持平台仍启用无前缀模式。

本机 OpenHarmony SDK 的 diff 位于 /usr/bin 之前，读取不存在文件仍返回 0，导致 Autoconf 将尚未生成的头文件判断为 unchanged。只在本次 Cargo 命令调整 PATH 使用系统基础工具，不修改用户配置或第三方源码。
