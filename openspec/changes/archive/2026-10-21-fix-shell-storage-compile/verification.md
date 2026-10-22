# 验证记录

## 最小修改与审查

开始时核对上一轮四个错误：当前工作区已存在 HistorySnapshot::reseed_event_counter，保留不覆盖。实际编译另暴露 history/projector.rs 将 Vec<ContentPart> 当作 Display 的类型错误。

三处生产行级修改：
- persistence.rs 删除 PersistedInfoLight 不存在且调用方未消费的 history 初始化；storage 层的 history 和 writer reseed 不变。
- jsonl/ledger.rs 的 append_journal_fact_sync 改 pub(super)，仅供父模块两个同步入口访问。
- history/projector.rs 的 User 文本分支改用既有 ConversationItem::text_content；沿既有文字提取规则，channel 不匹配仍失败。

独立 Luna 只读复核确认以上接口修正。未扩展存储迁移、不新增数据结构、持久化格式或行为契约。属于编译接线修正，没有新增镜像实现的测试。

修改前保存三个文件快照，修改后逐文件比对只包含上述删除一行、替换两行。没有 reset/stash/checkout、全仓格式化或覆盖其他未提交内容。

## 检查

- 修复前 cargo check --locked -p cli：失败，重现缺失字段/父模块可见性/用户正文类型错误。
- 最终 `PATH="$HOME/.cargo/bin:$PATH" CARGO_BUILD_JOBS=2 cargo check --locked -p cli`：exit 0，46.42 秒；Shell、Pager、Pager-minimal、CLI 均完成。
- `git diff --check`：通过。
- OpenSpec strict：归档前 19 passed。

归档完成后 strict 全量 18 passed、归档校验 682 passed。清理仅限 Shell/CLI 包构建产物，保留其他包缓存与工作区源码。未安装或替换运行中的 grow。
