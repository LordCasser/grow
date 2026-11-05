## 1. Evidence

- [x] 1.1 核对指定目录/summary、原运行期最后事件、三次unknown与后续load失败时间线。
- [x] 1.2 核对Shell生命周期/退役路径及触发证据，区分移除原因与移除结果。
- [x] 1.3 两个独立prefix复现加载后fatal，四事实控制定位late whole-session recovery；保留原首次故障尚未定因的限制。

## 2. Decision

- [x] 2.1 确定历史Workflow精确结算契约与ChatState/Shell所有者，不伪造原session恢复成功。
- [x] 2.2 严格校验delta并按依赖派发两个单owner增量。

## 3. Implementation and review

- [x] 3.1 ChatState owner提供Workflow-only持久结算，验证live Turn保持、epoch/child/checkpoint与幂等。
- [x] 3.1a 补独立review指出的多目标部分成功后失败恢复回归：保留首项ACK，失败项未接受，从真实持久前缀重建owner后重放同批不重复首项并结算剩余项。
- [x] 3.2 Shell owner在spawn前冻结targets并传递至childreconciler，替换无界恢复，补真实reconciler与新通知Turn并存回归。
- [x] 3.3 复核最终diff与独立review，隔离集成验证新通知可消费/actor持续健康及旧Workflow正确收尾。
- [x] 3.4 原prefix隔离复查一次，记录childfixture/原firstfailure限制，同步docs开发说明。

## 4. Closeout

- [x] 4.1 归档前核对每个场景及证据，严格全量校验、归档及archive校验。
