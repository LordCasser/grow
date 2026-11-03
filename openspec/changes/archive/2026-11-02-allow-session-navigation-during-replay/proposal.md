## Why

AppView 的 replay 输入保护先于 session 导航，导致加载中的页连 Ctrl+\、上一/下一 session、frame 返回点击也被吞掉。允许导航后晚到的 load failure 还会无条件返回旧来源并抢焦点。

## What Changes

加载期间保留注册的 session 导航和 attached frame 导航；正文编辑、提交、停止/配置命令继续等待历史完整。失败只回收其拥有的临时页，只有该页仍在前台才应用原返回位置。

## Capabilities

Modified: client-surfaces。

## Impact

Pager replay gate、load failure retirement、真实 AppView 输入链与回调测试。无恢复准入、actor 生命周期或历史存储修改。
