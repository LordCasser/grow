## Reproduction
旧 agent 实现运行 disabled_config_distinguishes_plugin_and_native_identity 失败：配置 demo:shared 后该插件条目仍 enabled=true。源码同时确认裸名称标记会匹配所有同名条目，Pager 原按钮也发送裸名称。

## Validation
- agent prompt::skills：91 passed。真实临时项目与插件目录覆盖限定键和裸键的隔离。
- pager --lib skills_toggle：2 passed，包含实际插件开关按钮派发与原生开关目标。该构建同时编译 Shell（pager 依赖），未运行 ACP 修改真实用户配置的端到端测试。
- macOS linker 报既有 __eh_frame 大小警告，编译及测试成功。

## Scope
不修改真实用户配置。裸 disabled 项现在仅指原生技能，不迁移已有列表。使用既有 dedup_key，没有增加另一套身份或兼容层。已安装 grow 未更新。
