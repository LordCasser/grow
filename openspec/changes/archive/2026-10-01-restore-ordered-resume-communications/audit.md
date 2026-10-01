# 有序恢复审计

用户指定来源的 12 条 Agent reply 在 Timeline 中存在、在 display cache 中缺失。尾部一堆 COORDINATION 的原因是 root 在历史加载之后整批发布 receipt；child 和离线 reader 也有单独尾部补录路径。不能隐藏全部通信或把普通 resume 换成 Replay。

| 情况 | 处理与验证 |
| --- | --- |
| 缺失 receipt | 统一 storage planner 按经 Timeline 核对的 canonical response/input/tool/通信身份定位；原物理行不重排，保留通信原样式和稳定 identity。 |
| 已有 receipt / repeated reload | 原行位置保留；重复 parent receipt 缓存行去重；合成行不写回，不创建新的 delivery。 |
| 增量 cursor | 任何合成或去重使 reconciliation changed，走既有 full replace；合成 meta 没有 eventId，不充当 physical cursor。 |
| 接收方 inquiry | 只补缺失 received/approval/completed；source peer + inquiry ID + phase 区分来源，既有阶段通过 Timeline identity 成为锚点；不信任错误缓存 timelineEvent。发送方审计不扩入本次。 |
| 无锚点 / 缺正文 | 有界尾部降级标 historyOrderEstimated；正文沿既有 immutable payload 校验与 unavailable 语义，不以文本/墙钟猜位置，不执行历史补齐。 |
| 大历史 | Notice 才做完整 decode，大工具正文用薄 peek；receipt seq 单调使第一 greater anchor 只向右扫描，即使物理 anchors 非单调也不重排，每行最多扫描一次。 |
| rewind / late append | 先用原 rewind/admission planning，再合并 retained Timeline 通信。真实来源仍活跃，校验原字节前缀不变，允许独立正常追加，不能把整文件 hash 变化误判成 reader 写入。 |
| loading 期间新 receipt | 实时 passive gateway 路由不依赖历史整批 snapshot；保留当前 pending inquiry 发布，禁止再次发布全部 receipt。 |

本次不新增 viewport 持久化，不改 Goal 自动恢复政策；source 真正 resume 会运行已有 Goal，所以真实用户来源仅做 pinned history 读取，运行行为在隔离 fixture 中验证。
