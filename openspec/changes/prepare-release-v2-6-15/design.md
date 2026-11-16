## Decision

使用现有发布链路，不建立本地替代分发。功能实现已在 main 工作树验收；发布 owner 只同步版本、文档、CI 登记和格式。格式变更保留 Rust token 和解析语义，不混入算法调整；新增 gap 修复由独立 Scrollback owner 与 delta 管理。

当前远端 main 为733d2c72e，与本地基线一致；正式最新为v2.6.14。创建v2.6.15注解tag前须保持候选已提交且干净，运行 scripts/validate-release.sh。main/tag 推送到 origin LordCasser/grow，默认分支 dispatch release.yml，所有平台组和publish均为true。官方流程固定tag commit，回归门禁失败不得绕过；公开发行须包含十个归档、SHA256SUMS和对应attestation。

## Review Plan

复核版本仅更新workspace继承包，release notes只列已验收行为并说明contextUsedTokens更名；验证文档链接、OpenSpec全量/归档、Rustfmt、默认CLI与all-targets检查，以及本次定向恢复/锚点测试。既有完整模块/PTY验收可复用，新增修改按风险补验。发布完成后记录运行URL、tag SHA、资产集合、checksum和可本机执行产物的version/help结果。
