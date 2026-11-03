## Why
算法C4：健康请求在durable完整性检查中复制Surface并丢弃，紧接着request builder再次复制同一内容。

## What Changes
完整性helper返回可复用的健康owned副本；确实修复时仍先按原路径提交、确认，再让builder复制已提交Surface。用户消息caller丢弃可复用副本，保持其复制成本。MemoryContext仅ACK成功后进入请求owned副本；Goal和image仍只投影请求。

## Impact
内部副本所有权复用，不改变行为、格式、权限或持久契约，skip_specs。Tier2真实request-time repair/memory ACK/失败取消及完整输出对比，健康成本探针及ChatState全量和独立review。
