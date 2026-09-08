# Design
使用已有 CancellationToken 与 biased select 包围 is_stdio/http_server_configured。调度时取消返回 false，循环中取消直接结束，已取得的标记由既有守卫释放。真实生产配置探测在 actor/mcp.rs 等待异步锁后同步读取禁用列表；此修改只使异步等待可取消，不声称能中断同步磁盘读取。

MockActions 加入可控制的未完成探测，测试 stdio/HTTP × 调度/循环四个位置。虚拟时钟推进 stdio 退避，Notify 证明已进入探测后再取消，不靠任意 sleep 猜测时序。验证无恢复调用、无状态推送、无残留标记。
