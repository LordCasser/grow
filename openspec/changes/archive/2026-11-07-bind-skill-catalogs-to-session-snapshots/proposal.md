# Proposal

## Why

AC-03b已定位到具体消费链：Pager Skills列表和toggle已携带会话身份，执行effect却丢弃它并发送cwd:"."；Shell又把进程shared latest插件快照与请求cwd混用。同cwd但不同session pluginDirs也会显示或校验到别的来源。模型实际技能owner是每会话SkillManager，不能用另一份全局目录覆盖它。

## What Changes

为现有Skills catalog/config请求增加可选sessionId上下文。有sessionId时取精确resident primary的执行cwd与其owner确认的plugin snapshot；无sessionId时保各方法已有cwd必填/默认规则，按该cwd构建只读registry。Pager列表/toggle发送sessionId并以原session和bindingepoch限定回包。全局disabled政策、fresh UI目录用途、现baseline刷新链和发现worker预算保持。

## Impact

Shell skills请求解析与既有SessionHandle::plugins_list错误传播；Pager现列表/toggle effect及回包资格；configuration-rules和client-surfaces行为。没有新服务、持久字段、第二runtime目录或watcher框架。AC-03a的LSP bootstrap和AC-06的直接config事件交付分别处理。本change目前只有设计，尚未实施或验证。
