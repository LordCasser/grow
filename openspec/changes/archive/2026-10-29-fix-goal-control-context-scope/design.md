## Decision
在同一actor durable Control命令内随已确认的Control提交切换请求scope；失败写入保留旧投影，不增加第二份持久化权威或独立ACK。Shell共享Control提交入口计算现有Goal资格谓词，cold bring-up保留现有初始化。

## Verification
先构造生产入口反例，再验证成功/错误/恢复与重复操作；持久化事实不靠内存pending缺失推断。

## 请求准备交接
Shell主请求在读取active Goal tag后还会await图片资格查询，再向ChatState入队。异步Control提交期间旧tag可能晚于新Control入队并反向覆盖scope。用既有step_control_gate包住tag读取与build_request调用；它与Goal控制入口已有相同边界，不新增epoch、来源标记或第二份授权。投影结束后释放gate，压缩/结算/派发继续使用原流程。
