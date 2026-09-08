## Evidence
全仓 Rust 的 save_config 符号检索只有定义、文档引用和注释。save_config_locked 仅由 save_config 调用。update_config 当前持有 SAVE_LOCK，调用 update_config_at -> read_config_for_save -> 修改闭包 -> save_config_at。settings_writes 的 UI/session 设置函数均走 update_config。

## Decision
候选仅覆盖公开 save_config 与它专用的 locked 包装，保留所有内部读写 helper、锁、update_config 及 tests。公开入口通过 util/config/mod.rs 的 pub use persist::* 暴露，因此无仓内调用不等于能证明仓外无人使用。未经确认不删除。
