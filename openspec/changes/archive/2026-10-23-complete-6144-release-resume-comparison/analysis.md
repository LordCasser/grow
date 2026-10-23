# 6144轮官方release端到端补测

同一台Apple M4 Pro（12核、24 GiB、macOS27.0.1），两个官方macOS ARM64 release-dist版本，同等6144轮现代admission纯文本根会话，每轮约32 KiB assistant正文，合计约192 MiB。原256 MiB夹具预算未变，旧版经既有test-only exporter生成其支持的Timeline+updates，新版为统一journal；以逻辑内容等价衡量，不为追求相同文件字节人为填充。

## 结果

单位秒，中位数[min,max]，每版5个正式样本。起点为PTY spawn前，50×120终端，诊断off，独立HOME/GROW_HOME/cwd与目标进程，本地mock。夹具生成、完整wire oracle和摘要在计时外，页缓存温热。首历史为首个含正文的完整模拟终端帧，未要求恰好显示第一轮；末屏为最终唯一marker完整帧。可输入在全历史/Loading消失后发不提交草稿，观察完整帧回显，是包含按键/渲染传输的外部输入就绪上界，不是模型采样耗时。

| 指标 | 2.4.3 | 2.5.0 | 耗时下降 |
| --- | ---: | ---: | ---: |
| 首历史可见 | 2.982 [2.974, 3.234] | 0.866 [0.859, 0.964] | 71.0% |
| 末屏可见 | 6.595 [6.536, 6.767] | 1.304 [1.220, 1.327] | 80.2% |
| 可输入（草稿回显） | 16.386 [16.219, 16.756] | 1.370 [1.314, 1.413] | 91.6% |

可输入减少约15.02秒，约11.96倍加速（耗时下降91.6%）。首历史约3.44倍，末屏约5.06倍，三者不能混为同一指标。最终聚合、版本与observer身份见[results.json](results.json)。较短规模沿用[前轮有效报告](https://github.com/LordCasser/grow/blob/main/openspec/changes/archive/2026-10-23-compare-release-resume-v2-4-3-v2-5-0/analysis.md)；本轮6144独立重建/重测，未拼接前轮部分样本，不能据跨时段数据推导精确增长曲线。

## 有效性与对抗检查

- 重新下载两份官方包，核对SHA256/checksum/API digest/size、指定workflow/source attestation、codesign strict和隔离环境version。2.4.3源2069c43，2.5.0源8443a4d；executable SHA与前轮一致。观察器使用Rust1.93.1 ordinary release，不替换待测官方release-dist；源SHA与前轮已验证源码完全相同，复建零warning、fmt通过。
- 本轮从pair0重新预热，两份真实CLI各执行完整stdio session/load oracle，12,288条角色/正文/顺序与期望BLAKE3一致。所有正式样本逻辑201,577,386正文bytes、摘要536552b49bfb66dc3bc9da928ada88d2e8e4587db90ef5a2c984c3cd9a39fe5c一致。所有样本推理请求0、清草稿后Ctrl+D退出0。
- 用户确认暂缓并行编译；自有构建结束才采样，首对前等待10秒安静窗口，0.5秒间隔监测。pair0排除预热，pair1..5按BA/AB/BA/AB/BA配对，全部首次通过；所有正式pair编译监测0命中。源、observer与两个目标binary SHA每对核对一致。没有因耗时选择样本。
- wire oracle承担完整正文和顺序校验，末条marker仅承担末屏计时；闭合同步更新帧后再取屏幕。未放宽加载限制、未跳过校验、未调低持久化等级，不做就绪与历史分页分离。

仍然是本机、温热页缓存、纯文本合成根会话、n=5证据，不是p95或跨硬件/冷盘/图片/工具/子会话收益保证。机器非独占，监测无法排除短于0.5秒的活动；模拟终端观测也含接收和消费成本，不能声称物理显示器时刻或纯RPC延迟。原始cast/日志/下载/临时脚本只留本轮核对并在收尾清理，不入仓。
