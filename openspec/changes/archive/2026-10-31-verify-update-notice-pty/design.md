## Approach

测试放在 CLI 组合层，像生产入口一样安装 pager-minimal 渲染 hooks，避免 pager 反向依赖 minimal。集成测试以自身 executable 的 ignored 子测试启动 Pager，复用 PtyHarness、ContentController 和隔离 TestSandbox。父进程等待真实 UI 就绪后通过隔离目录的触发文件放行子进程向既有 oneshot 发送结果。避免启动安装器或请求线上发布服务；不向生产入口增加测试开关。

检查终端模拟器可见文本，保存 HTML 与纯文本快照；无更新用 None 结果确认没有误报。此测试覆盖检测结果交付到 UI 的端到端显示，不覆盖线上版本检查和下载。
