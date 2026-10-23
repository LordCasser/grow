# 验证记录

- 本轮不修改Rust、Cargo依赖或产品行为。观察器源码SHA256 `61ae842415aeb1bf9ad0e0e78c68bd8631a514c9043fb3a58e3c714718c73ac1`，与上一轮已通过strict Clippy的源完全相同。本轮另用固定Rust1.93.1和专用target重建普通release观察器，构建成功且compiler-message warning为0，fmt check通过。
- 目标为官方release-dist 2.4.3/2.5.0，archive/checksum/API digest/size、指定workflow/source的provenance attestation、codesign strict及独立HOME/GROW_HOME版本输出重新核对。未用本机观察器依赖编译出的Grow替代官方目标。
- 原6144部分样本已清理，不拼接历史采样；新建隔离会话/子进程，pair0重新完整wire oracle，逐角色/正文/顺序BLAKE3一致。每样本同逻辑12,288条、201,577,386正文bytes，全部推理请求0；末屏/Loading消失后实际草稿回显，再清草稿和Ctrl+D退出0。源/二进制SHA每对前后保持一致。

## 对抗审查

完整wire oracle承担内容与顺序验证，最终marker只承担末屏可见计时；不把单个marker当作全历史正确性的证明。首历史为首个含正文的完整模拟终端帧，不要求恰为第一轮；输入回显是全历史后可编辑输入的外部上界，包含按键/渲染传输，不是模型首token时间。只观察闭合的同步更新帧，不把部分接收字节当作完成绘制。

每对原定BA/AB顺序，编译污染整对排除，不基于耗时选择；不跳过校验、不调低持久化等级，不做就绪与分页分离。纯文本根会话、温热页缓存、非独占机器和n=5仍不支持跨硬件/冷盘/混合媒体收益承诺；0.5秒采样也无法排除更短的环境活动。未提高原256MiB夹具预算。Luna独立只读复算确认12行=2预热+10正式，两个版本各5正式，采样顺序正确；所有median/min/max/reduction/speedup与results.json一致（1e-9浮点容差），可输入下降91.6419699394%，speedup11.9645417970。计数、payload/顺序摘要、pair监测状态无不一致。公开release420909068正文已逐字读回匹配本地notes，11份资产的id/name/size/digest/download URL均与修改前相同；标签对象仍f1e99dff7af3b499f8c8d432ba7e9d745b7c8829，仍为latest公开非prerelease。没有重打tag或重建产物。

归档前全量strict校验17项通过，diff check通过。最终source/observer/目标binary SHA再次匹配，清理前无本轮存活Grow或observer。专用target占4,174,479,360 bytes，临时根293,494,784 bytes，已全部清理（约4.47 GB），包括下载、cast、日志、临时脚本、独立HOME/GROW_HOME及指针，删除后确认不存在。保留原target/local-release-2.5.0候选，SHA核对不变；不清共享target。归档后全量strict校验16项、archive校验704项全部通过，0失败。提案采用标准Why/What Changes结构。
