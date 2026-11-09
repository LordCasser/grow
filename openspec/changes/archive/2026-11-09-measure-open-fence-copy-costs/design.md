## Measurement

固定 YAML 每行增长，256、1024、4096 行；先走真实 OpenFenceHighlighter 的每行追加，统计逻辑返回文本字节。独立使用相同 syntect parse/highlight 操作归因，不用不等价总时间相减。缓存 clone 与 prefix 检查分别重复 16 次取均值；真实 StreamingMarkdownRenderer 每次先建立前 N-1 行再测最后一行增长，重复 8 次记录中位数与最大值。语法资产初始化在计时前，输出与 batch 完全核对。

以优化构建运行，不把 debug 编译测量当产品帧延迟。当前测试只覆盖单一 YAML 合成负载，未覆盖真实终端、draw、宽表格、图片或 UTF-8 复杂度；不设置不稳定的 wall-time CI 断言。若测试暴露帧预算影响，后续独立 delta 明确最窄输出借用边界。
