# Design

Pending 重复 Quit 继续合并，保留同一次请求和五秒期限。Unconfirmed 时重新走原 Quit 快捷键的两次确认：第一次只确认，第二次派发现有 RetryExitSuspension，创建新 transport exit_id/五秒期限；Shell 继续核对原 suspension operation，不重放工作。显式 r 重试、f 强退保持既有语义；全局退出恢复按键优先于局部行为确认。过期旧 TaskResult 仍按 exit_id 拒绝。

未确认提示在 Welcome、Agent 和 Dashboard 都可见。把动作提示保留在独立底行，错误文本只占其余行；窄终端使用短英文 r/f 提示，不能被长错误换行挤掉。无可用绘制区域时不越界。Pending 不产生多余请求，Unconfirmed 不禁用原有编辑/取消路径。

通过真实输入→dispatch→失败结果→双击 Ctrl+D→retry→旧响应拒绝→确认退出的测试覆盖跨层状态，不仅直接调用 action。增加窄窗口/长错误、Pending 合并、Ctrl+C 不变的回归。不声称仅凭代码已确认用户看不到提示的唯一原因。

复核独立 minimal 渲染链确认：AppView::draw 提前转交 hook，原先完全不绘制退出状态。Minimal 复用同一 painter，通过现有 minimal_api 薄接线，不维护第二套文案。退出状态存在时优先显示该区域，避免 startup/面板/modal 早退遮挡；viewport 为动作行与错误保留空间，仍受现有终端高度上限约束。

最终逐项检查所有绘制 return 时，又确认普通 Agent 分支也提前返回，因此“只在公共尾部加 painter”原本对主要页面不生效。正常视图将内容绘制放在一个内层 callback，统一在外层完成退出 overlay 后才返回，不在各个 return 前分散补调用。子进程回归执行真实 AppView::draw 并捕获 Agent/Welcome/Dashboard 的输出，防止共享 painter 单测掩盖接线遗漏。
