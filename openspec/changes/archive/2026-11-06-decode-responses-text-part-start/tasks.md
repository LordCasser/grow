## 1. Evidence and contract

- [x] 1.1 只读核对指定会话两次失败与成功对照、锁定SDK和实际client/L2边界，明确下一版本范围。
- [x] 1.2 合成无副作用HTTP/SSE回归，在旧decoder上保存有效red。

## 2. Isolated next-version repair

- [x] 2.1 仅规范化初始空output_text added part缺省annotations，保留原evidence和其他SDK拒收。
- [x] 2.2 完成client→L2 red/green、已有annotations/错误类型/重复key/done/terminal/EOF控制及相关Sampler回归，固定1.93.1检查零warning。
- [x] 2.3 父核最终diff/证据，记录下一版本验收，同步开发说明、strict校验并归档；不改变2.6.1tag或发布下一版本。
