# Deliver Skills config reloads

## Why

AC-06：ConfigReloader 已产生 Skills 更新，但 leader 只记日志，in-process 路径忽略；Skills watcher 只在启动时消费 custom paths。只编辑 config 的 disabled/ignore/paths 不会交给现有 SkillManager，新增自定义目录也不会被继续监听。常规 UI toggle 自己广播 refresh，不能替代本问题。

## What Changes

leader 和 in-process 各自将配置更新交给既有 reload owner，并让各自已有 discovery watcher 采用最新 paths。仍由每个 Session 的 reload_skills_from_disk 使用 fresh config+自己的 plugin snapshot 更新 SkillManager/广告；配置 runtime 不自建目录或模型可调用状态。两条运行入口按模块分别实施，复用现 watcher 构造/登记与取消回收规则。

## Impact

Shell leader app 与 Pager in-process spawn，以及实际配置文件事件回归。无新协议或用户配置项，不改变全局disabled身份、session快照或AC-03b API归属。
