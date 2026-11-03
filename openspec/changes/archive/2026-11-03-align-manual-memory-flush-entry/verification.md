# 验证

- `auxiliary_load_tests`：3 passed。实际 extension 调用等待load publication、closed channel返回失败、false checkpoint不返回空成功。
- `manual_flush_slash_admits_while_foreground_owns_a_turn`：真实regular foreground owner下接纳slash flush，HTTP与文件成功，不写入模型input队列。
- 真实ACP `memory_flush_sideband_completes_while_foreground_request_is_held`：held foreground期间flush先写实际Markdown，随后释放主请求并正常结束；headless prompt `/flush`亦落盘。
- Full/Minimal真实PTY `/recap`与`/flush`：通过；只在memory/**/sessions/*.md中核对唯一marker，不把初始化MEMORY.md当结果。
- 全部图片模块54 passed；完整ACP旅程5 passed、PTY旅程3 passed。全量严格OpenSpec和限定文件diff-check通过。
