## Approach

以当前 OpenSpec 行为契约为权威，分别核对后端 phase/authority、客户端 preview/comment/prompt 状态和实际模型请求。审计记录列出每项场景对应的生产入口与验证，不以工具链格式校验代替运行结果。

审批操作栏复用 `modal_window::shortcuts_rows_needed/render_modal_shortcuts` 的多行布局与点击区域；内容 viewport 扣除实际 footer 高度，窄窗口不再隐藏整排动作。提示必须和当前焦点路由一致，未确认决定仍禁止冲突操作。

Minimal 保留审批正文进入 native scrollback、底部承载审批操作的既有设计；Commenting 与 Prompt 都显示实际编辑框。没有 live approval 的普通/retained/已完成详情，沿既有 Minimal modal 分派复用同一个 line viewer，避免出现输入 owner 存在但未绘制的状态。只通过 minimal_api 暴露必要读写视图与 test-support 夹具，不复制 Plan 生命周期状态。

普通预览在无 live approval 时发普通 prompt 的既有语义不暗改为批准或抢占执行；明确展示发送/排队含义。retained candidate 仍需 `Review` 回到原审批入口。

使用 mock provider 的真实 PTY 串起批注/自由文本修订、确认自动收束、再次提交、批准与完成；检查请求中的反馈和窗口状态，避免只匹配一句模型输出。其他环境失败明确记录，不能删掉失败场景。

存储失败回归通过真实 amend/cancel 工具入口注入 artifact 写失败和永久 Control ACK 失败。永久持久化故障允许 writer 关闭并向上返回错误；不要求不可写的时间线伪造一个工具终态。验证原 live 与已确认 Control、artifact 和权限未变，且没有成功回执或退出 Normal 的投影。
