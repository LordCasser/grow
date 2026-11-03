# 验证

2026-11-03，隔离GROW_HOME与RUST_MIN_STACK=16777216，Cargo均--locked。

- `cargo test -p shell --lib task_resume_presentation_tests -- --test-threads=1`：1 passed，外层驱动Completed/Cancelled × create/cold四个独立OS进程。真实coordinator源Spawn、provider思考/正文/工具、人工批准或Cancel、canonical终态、resume_from派生及新child等待；live Spawned回调即按Pager读取入口观察完整继承前缀；源journal bytes不变。独立cold reader与stream一致、源行仅一次且先于新prompt；ToolCallUpdate全字段参与比较，Completed工具结果/终态明确断言。最终补强运行7.39秒。
- Shell全量库回归：4473 passed、1个新fixture失败、9 ignored（手动基准），225.88秒。唯一失败是新fixture误认为不同lineage的SurfaceId数值必不同；在源消息前添加合法不影响Surface的SessionTitle以推进坐标，保留新Seed所有权与坐标转换断言。没有为通过测试改生产行为。失败项最终重跑结果见下方补记。
- 全量覆盖了共用copy的double-fork、reasoning stripping、rewind截断、source权限清除、原子child publication错误、history projector，以及恢复权限/规则/30阶段child restore等既有回归。全量后只修改该fixture构造，不重复整个suite；单独公共fixture的工具更新补强已另行通过。

`resume_seed_presentation_rebinds_display_to_the_new_seed`在真实staged journal与独立reader中核对：prepared User echo不同于模型正文、非active旧display inline、新Seed Assistant/Reasoning引用、source坐标/cursor/响应/ingress元数据清除、当前状态不复制、读取与stream顺序。old-inline是构造的prepared输出，并非真实compaction；实现复用现有validated projector，不声明本次测量或真实compaction全链。

独立review最终通过，git diff --check通过。主规范/开发说明和归档校验随最终补记。

最终失败项定向重跑：1 passed，0 failed，0.06秒。重跑runner一度停在macOS dyld启动层（采样仅_dyld_start，未进入test harness）；系统验证恢复后正常执行，无测试重试或生产超时修改。最终源码所有已执行非ignored检查通过：全量4473项加修正后的唯一fixture；9项manual benchmark仍未运行。公共fixture工具更新补强的另次运行亦通过。

归档前strict18/18；归档后全量strict17/17、archive tasks786/786通过。
