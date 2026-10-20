## Design
使用下一个 patch v2.4.3。先完成本地相关回归和规范归档；提交源码/审计与版本准备后，创建指向清洁 HEAD 的 annotated tag，运行 release preflight，原子推送 main/tag，从 main dispatch 官方 release.yml，四组平台全选且 publish=true。

工作流在同一验证 SHA 执行 Core（含完整 Settings integration）、三平台协调/导出、Windows storage、OpenSpec 和十平台构建。全部完成后核对公开 non-draft/non-prerelease/latest、十个 archive 与 SHA256SUMS、远端 tag，以及下载的 macOS ARM64 包摘要、签名和隔离 GROW_HOME 的版本身份。实际发布结果另追加验证记录。
