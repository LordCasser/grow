## 不变量证据
ImageBudgetSelection唯一构造/insertion在request builder：new空IDs/空map，eviction从当前items取得offset，最终写满完整current IDs后才存入state。builder复用需route/activeGoal/useDescriptions及currentIDs.starts_with(savedIDs)；pressure复用也要求该prefix。append保持旧item位置，rewind/replace/compaction改prefix则不复用。Goal和图片描述投影只改item内容，不插删item；Weak身份继续判断同坐标part。cache初始化None且不序列化，无迁移兼容层。

压力刷新在goal scope变化后可能暂沿旧选择，只有下一次builder完整检查domain；本次不改变这个既有边界。每项仍比较savedIDs[index]与currentIDs[index]，不能把offset误当wire删除/降采样之后的位置。

## 实现
replaced key从(SurfaceId,part)改(index,part)，复用已有surface_ids做绑定，没有第二索引。验证/应用只访问目标K项；新增eviction直接用已知index。全部合法性检查后，BTreeMap位置keys即原Surface顺序的effective_evicted_parts，可直接收集，消除全量反查。真实SourceId/Weak检查不删。

## 验证
Tier2+独立review：旧按SID定位循环作reference，比较request全清与pressure best-effort的序列化item/part输出；K0/1、多source/同source多part、stale SID/新Arc/description identity/越界/非Image、prefix append。真实build_request的repeat/append/coldReset/newboundary及context-pressure route regressions保留。临时小图纯kernel基准在setup/clone外计validation+apply，不计effective-parts reporting，N128/2048/8192、D0/1/2/8/64；D>0分别测front与tail source位置，D0测一次，另测front/tail同source多part，报告原始样本与median。未选Surface位置为普通text；不把kernel收益称请求编码或provider速度。旧producer先绿后再改表示。
