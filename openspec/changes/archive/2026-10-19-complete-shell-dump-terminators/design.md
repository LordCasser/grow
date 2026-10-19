## Completion boundary

parse_dump 要求严格的 marker + newline。reader 只检查末尾完整独立标记行（Bash 或 Zsh），接收不完整标记后继续读，完整后不等待背景继承 fd 的 EOF。suffix 长度固定，累计处理随载荷线性增长；不再每个 4096 字节块重扫全部历史。保留损坏/超时快照不替换先前状态的生产语义。

## Validation

真实 os_pipe 先写 start、cwd、END marker 字符，保持 write fd，再由测试发送换行。旧 reader 会在换行前完成，修复版必须等待，随后即使 writer 未关闭也应返回可 parse 的状态。覆盖 Bash/Zsh。大函数用例继续两轮传输与恢复，失败时 test helper 只报告 dump 字节长度、标记完整性与时间，不输出编码环境内容。Ubuntu 的实际失败仍需最终同提交门禁确认；不降低载荷、不延长期限、不跳过。
