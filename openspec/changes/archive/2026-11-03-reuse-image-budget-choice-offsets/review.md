# 独立对抗审查

reviewer `/root/rules_recovery_review`只读核实源码/调用方/本增量diff，未跑Cargo。反证后支持offset key：cache唯一写者会保存完整surface_ids，两消费入口均有prefix检查；没有合法Source移位仍prefix相等的路径，没有持久/跨actor复制。Goal/description只改item内容，不插删item，Weak身份仍负责同slot的part变化。无需临时SID hash map、阈值或常驻字段。

最终实现复核通过：get+zip要求saved/current ID均存在并相等，items/part边界与URL/description Weak核对完整；builder单项invalid清全组，pressure现有prefix外壳与逐项skip保留。新eviction只在原items位置插key，更新完整snapshot之前不重放new choice；确认valid与new entries后按index/part key生成报告，顺序等于旧Surface扫描。非单调SID fixture验证不误用旧SID字典序。

Legacy oracle从独立specs+saved SID构造，没有从新map反造旧输入。最终静态review未发现剩余可行动问题。测量只能称定位validation/apply内核，prefix比较/clone/bytecount等仍另有成本。
