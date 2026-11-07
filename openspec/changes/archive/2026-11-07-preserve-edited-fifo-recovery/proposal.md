# Why

非延期backlog Session F2在当前源码仍可定位：live保存编辑在原槽位换新input；Timeline pending查询却按新的Submitted位置输出，冷恢复使A、B变成B、A′。恢复同时把编辑请求的prompt_id当运行prompt identity，和持久队列行id分叉。既有F1/F3修复没有改变这两处。

# What Changes

由Timeline既有supersedes事实恢复替换输入继承的FIFO位置；Shell恢复消费该顺序，并以既有StoredQueueEntry.id恢复原运行/展示行身份。编辑请求的稳定prompt/input身份继续负责不可变接纳与Hook幂等，不能把它改成原请求或复用旧payload。

本change只处理保存编辑后的恢复，不新增队列排序持久事件、不改显式reorder的现有会话内语义，不改Hook/图片或Goal仲裁政策。

# Impact

ChatState canonical pending query、Shell input reconstruction及队列编辑恢复测试；同步input-admission delta和开发说明。验收需要A/B/C多次编辑及真实持久边界冷恢复的旧红新绿，未执行前不计已修复。
