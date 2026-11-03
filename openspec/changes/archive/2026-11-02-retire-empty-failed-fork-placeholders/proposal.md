## Why

fork copy 请求失败后保留无 session id 的占位页，旧 code 又清掉 pending directive。页面既不是真实 session，也缺少明确恢复落点；worktree fork 的创建失败有同类状态。

## What Changes

未绑定的空失败 fork 页退休，并在仍是当前页时回来源（保留原 Dashboard attach）；晚到的后台失败不抢焦点。未发送 directive/草稿保留在明确的失败草稿页，清除启动/loading 状态并可按既有关闭流程保存/退出。不处理已绑定页为初始失败，不删除成功创建的副本；父级提示区分请求与成功。

## Capabilities

Modified: `client-surfaces`。

## Impact

Pager fork/session failure 回调、局部页面退休与测试。无后端 fork、历史复制或删除协议变化。
