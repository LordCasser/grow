## Design
保留四个原断言，补齐其精确缺项；Bool 默认值测试显式复位 show_model_provider 的线程缓存，避免新增交互测试污染后续默认断言。键盘/鼠标沿生产 settings handlers 验证实际 typed Action，不以登记名单替代交互覆盖。

审计范围覆盖 Settings catalog 全部条目的 current value、commit、reset、persist 和 rollback，UiConfig/用户偏好写入 helper，以及子 Agent 相邻独立配置。用代码和现行规范判定专用入口与实际缺口。结果保存在本 change。

现行 release 复用 Core regression，但 Core workflow 未执行 settings_e2e；增加整组 integration 执行，沿既有不可变 SHA、失败阻止发布契约，不建立新的门禁框架。此为测试/工具维护，不修改产品契约。

完整 settings_e2e 以及相关 library/保存回归通过后才记录完成；发布通过不可变源码 SHA 的正式十平台工作流。
