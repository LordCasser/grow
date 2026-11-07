# Why

Exact Cargo runner 在 Unix 超时/取消时只向本次启动的进程组发信号、回收直接子进程，却把组信号成功、直接子进程结束和输出EOF当成完整进程树已退役。已实跑反例中的setsid＋DEVNULL后代仍活跃，而runner没有报告cleanup unconfirmed。现有development-workflow契约允许明确未确认，不能用不足的观察宣称已清完。

# What Changes

保留现有有界组信号与直接子进程回收。Unix被迫中断的phase明确报告：该机制无法确认脱离进程组的后代已结束；不再输出完整tree confirmed。正常完成、精确选测试、单产物/三阶段/deadline语义保持。用真实普通后代、setsid持pipe与setsid DEVNULL的超时/取消对照保留边界。

本项不引入跨平台进程容器、全局进程枚举或按名称/PID文件杀进程；实际owned fixture由测试持有的活句柄/本次自建组完成finally清理。Windows现taskkill分支保持，未实跑的边界明确记录，不从Unix结论扩张改动。
