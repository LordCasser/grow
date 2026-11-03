## Why

按已完成的 Dashboard UX 对齐分析实施，消除同一 session 因入口不同改变编辑规则、总览隐式更换收件人/清稿、操作语义重载及过滤改变详情切换的问题。前一轮 runtime/writer/删除保护保持不变。

## What Changes

逐个可运行增量：统一普通/attached 详情输入；总览改为只读预览和显式新建，搜索独立保稿；统一配置前缀并分离停止/关闭/永久删除；打开会话顺序稳定且独立于总览过滤，明确来源/活动/焦点。完成后独立 review，再补齐发现的盲区。

## Capabilities

Modified: client-surfaces。

## Impact

Pager 输入路由、Dashboard 状态/呈现/动作、ActionRegistry、相应真实输入与 PTY 测试。无 Shell 生命周期或持久化接口变化；不操作真实 session、不安装全局命令。
