## Scope

以 v2.1.5 为发布基线，优先审查新采样 attempt 的计费/准入/撤回/持久化链，兼顾父子通信、Goal 生命周期、用量 UI、压缩续接、Timeline replay 和 Workflow resume。保留所有既有工作树改动，不把历史验证结果当作本次执行结果。

## Release

复用 Cargo workspace 版本、Cargo.lock、shell/changelogs 及 scripts/validate-release.sh。版本递增为 2.1.6。完成本地验证后提交，运行官方 CI；使用指向同一已验证提交的 annotated tag，执行 release.yml 的全部平台及 publish。确认十个归档及 SHA256SUMS 发布后报告真实结果。

## Validation boundaries

本机 macOS 回归验证本地路径；Linux 核心回归、跨平台 coordination、Windows 存储由既有 GitHub Actions 验证。真实供应商收费和远端副作用不由 mock 测试保证。平台或发布失败必须保留日志并修复或明确报告，不能将工作流已启动写成发布完成。
