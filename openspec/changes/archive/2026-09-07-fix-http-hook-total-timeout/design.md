# Design
run_http_hook 调用私有 run_http_hook_with_validation，生产传固定 validate_hook_url，测试可注入带延迟校验以稳定覆盖阶段预算。内部执行 future 使用单个 tokio timeout 包围，timeout_info 在 URL 展开后及收到状态码后更新；timeout 丢弃 future 再读取最后元信息。去掉独立 DNS timeout。

同步的配置展开、客户端构建和 JSON 解析不具有抢占性，契约限于可挂起的异步等待；不宣称硬实时中断同步代码。测试使用本地明文 HTTP 和受控校验函数，不修改生产 HTTPS 校验。
