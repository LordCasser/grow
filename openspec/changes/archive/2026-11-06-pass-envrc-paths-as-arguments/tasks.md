# Tasks: pass-envrc-paths-as-arguments

- [x] 1. 核对 WS-05 证据、`load_envrc_via_bash` 现实现与三个 trusted 调用方；确认职责在 workspace envrc 加载器。
- [x] 2. 建 proposal、delta（WHEN/THEN）、design、tasks；`openspec validate pass-envrc-paths-as-arguments --strict` 通过。
- [x] 3. 实现：删 `cd` 与路径插值，固定 `. "./.envrc"`，`current_dir(dir)` 加 `env PWD=dir`；`set -e`/stubs/`env -0`/基线过滤不变。
- [x] 4. 现有 tests 新增特殊字符目录真实加载对照（`$(touch marker)`、反引号、单双引号、空格/换行、非 UTF-8），既有回归不改；自文件 rustfmt 与 diff 自查完成。
- [x] 5. main 共享 target 的 envrc 测试最终 8/8、exit 0，父已核输出及开发说明。`test_non_utf8_dir_name_loads` 在本机因 EILSEQ 无法创建 fixture，仅该文件系统前提明确失败时未执行 loader；此场景的平台限制见 verification.md。
