# 验证记录

## 范围与正确性

仅新增被ignore的手动PTY对照用例、测试依赖和发布文档；不改生产恢复、持久化或输入状态机。正式比较使用官方发行包，不能把观察器自身的release构建误当作待测Grow。

- 官方两版下载包的checksum、API digest/size、source attestation、codesign和隔离环境version通过；身份保留在results.json。
- 每规模两版在排除的预热中执行真实stdio session/load，完整角色/正文/顺序摘要必须与逻辑夹具一致；每次正式样本同样核对逻辑数量/字节/摘要。
- PTY每次检查最终历史marker、Loading消失与未提交草稿回显；推理请求数必须0；清理草稿后Ctrl+D退出必须0。
- 复用原夹具预算，4096/6144轮未放宽生产读取或测试生成限制。旧版通过既有test-only exporter生成它支持的物理格式，不新增产品兼容分支。
- 观察器Rust 1.93.1 release编译通过；`cargo fmt --all -- --check`与针对新测试的`cargo clippy --offline --locked -p pager-pty-harness --test release_resume_comparison -- -D warnings`通过，无警告。
- 增加长样本时仅修改规模配置，measure和wire oracle代码与512/2048矩阵相同；两组分别固定观察器source/binary SHA，官方目标二进制相同。

## 对抗审查

不把末屏当作可输入，不把单条marker当成全历史内容正确性证明；输入必须有实际回显，完整内容另由真实CLI wire oracle验证。首历史定义为首个可观测历史画面，不能保证它恰好显示第一轮；同步帧闭合后才取模拟终端画面。该结果不是物理显示器扫描时刻。

合成文本、温热页缓存、n=5及非独占主机限制已在notes和analysis披露；不外推图片、密集工具、子会话或冷盘收益。外部编译污染整轮排除，不选快样本。正式窗口的0.5秒监测不能排除更短的其他进程活动。

没有就绪/历史分页分离，也未用降低持久化等级、跳过校验或提前输入换取性能。notes只报告官方版本的直接对照，不累加局部优化收益。

## 收尾

正式结果为512/2048/4096轮各五对，共30正式样本、6排除预热；全部逻辑摘要匹配，推理请求0、正常Ctrl+D退出0。512/2048最终整矩阵编译监测为空；4096正式五对各0命中（pair2污染两次整体重测），独立Luna只读复算确认中位数/范围/降幅/倍数。6144仅两对有效正式样本，另十对污染排除，未达到预定五对；完成当前pair、无子进程后停止，所有6144耗时不进入结论。没有为取得显著数字回收此前整矩阵排除的样本。

公开release 420909068正文已与仓库notes逐字读回匹配；11份资产的id/name/size/digest/download URL前后完全一致。v2.5.0标签对象仍为f1e99dff7af3b499f8c8d432ba7e9d745b7c8829，仍是latest公开非prerelease；不重建标签或产物。

最终source/observer/目标二进制SHA再核对通过，fmt check和git diff check通过，归档前全量strict规范校验17项通过（含本change及另一既有change）。专用target占用9,471,565,824 bytes，临时目录304,111,616 bytes；清理前无本轮存活Grow/observer进程。保留既有target/local-release-2.5.0官方候选（SHA再次核对一致），不清共享target。已删除上述专用target、下载包、cast、隔离HOME/GROW_HOME、临时脚本/日志和路径指针，总计约9.78 GB；删除后确认不存在，既有官方候选保持原SHA。归档后全量strict校验16项通过，archive校验703项通过，0失败。
