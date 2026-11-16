## Decision

IndexedHistory.tools已有#[cfg(unix)]，timeline_generation只用于证明Unix派生索引generation连续性，不是运行时API。将其从#[cfg(test)]收紧到#[cfg(all(test, unix))]，使观测接口与其能力的所有者同时出现。调用方timeline_catalog_test和timeline_suffix_test的实际mod准入须核对为Unix；若证据不同先修订这项假设，不在Windows制造返回0的观测结果。

## Review Plan

核对helper、字段、两个调用测试mod和cfg diff；在1.93.1复跑已存在的catalog/suffix generation用例与fmt。Windows通过标准为新候选官方storage及native门禁均真实编译/运行成功；未完成前只报本地局部验收。
