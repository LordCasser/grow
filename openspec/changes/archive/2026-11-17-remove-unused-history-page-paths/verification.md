# 验收

父亲读两个模块diff与全crate调用图：finish_display_page在删前无caller；prepend_entries_from只被新gap测试的旧分支使用。真实生产安装由history_window.apply_history_page/project_history调用replace_history_prefix；finish_transcript_turn与finish_all_running仍有正常生产调用，没有级联删除活跃finalization。

- 删除projection完整旧方法5行，保留finish_snapshot及实际Turn事件处理。
- 删除Scrollback完整旧入口28行及旧doc；gap测试直接走replace_history_prefix(page,0,true)，同EntryId、offset=virtual_y+height、next屏幕row1和capture身份全部断言保留。同几何、换宽和正文夹紧测试未动。
- 固定Rust1.93.1：`RUSTFLAGS="-D warnings" cargo check --locked --profile release-dist --features release-dist -p cli --bin grow`退出0，正式profile检查无警告。日志terminal/call_d14a51fe7ff44acdb8784d96cf369b0d.log。
- `pager --lib gap_anchor`4 passed、0 failed，`pager --lib transcript_projection`22 passed、0 failed。日志terminal/call_c223259da022477d9e5860aff8997a2e.log。
- 全仓fmt、git diff --check退出0。docs/development.md同步实际release-dist profile/feature和-Dwarnings本地检查入口，并明确它不替代正式同SHA跨平台门禁。没有allow(dead_code)、发布门禁改写、运行时兼容分支或生产语义调整。

本项纯重构可验收归档。后继正式patch仍须完整通过Windows分发与其他平台门禁，不以本机检查代替原生或发行产物证明。
