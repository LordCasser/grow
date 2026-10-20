# 发布准备验证

## 已完成的本地准备

子 Agent permission Settings 和四项 show_model_provider 清单修复的相关回归已通过：完整 Settings integration 231 passed、0 failed、0 ignored；Pager settings library 331 passed、0 failed、1 ignored；Pager 子 Agent permission 20 passed，Shell 相关 5 passed。组之间有交集，不累加；细节见 complete-settings-preference-test-inventory 验证。完整 Settings integration 已接入 Core regression，在正式发布时重新校验同一不可变 SHA。

workspace 2.4.2 → 2.4.3；Cargo.lock 只修改 33 个本地 workspace 包版本。按 TOML 对比将这些本地版本还原后，整份 lock 与 HEAD 完全相等，第三方依赖和 checksum 无变化。cargo metadata --locked --no-deps 确认 CLI 2.4.3。发行说明使用绝对链接。

Python scripts 34 项执行，33 通过、1 skipped；修改 Rust 文件 rustfmt --check、git diff --check 通过。OpenSpec strict 校验归档前 18 项 passed，0 failed。

## 发布步骤与边界

完成本地相关回归和归档后提交源码/测试/审计与版本准备；创建指向清洁 HEAD 的 annotated v2.4.3，运行 scripts/validate-release.sh，再原子推送 main/tag，从 main dispatch 官方 release.yml，全部平台启用且 publish=true。

准备归档只证明候选可供托管验证，不宣称公开发布已完成。跟踪全部门禁和十平台构建，通过后核对 Release 非草稿/非预发布/latest、十个 archive + SHA256SUMS、远端 tag commit、实际下载的 macOS ARM64 包摘要/签名/隔离配置版本身份，再追加最终结果。

归档后严格校验：Totals: 16 passed, 0 failed (16 items)；Totals: 663 passed, 0 failed (663 items)。

## 首轮候选的 Windows 夹具失败

候选 578d0fb2232e5286e26dc62ccfbbed2df21b7a25 与 annotated tag object f3a28be37f65ed81f37444609a05dcfa3fab7f2b 已通过静态 preflight 并推送。官方 [run 39422108840](https://github.com/LordCasser/grow/actions/runs/39422108840) 中完整 Settings integration 已 success；Windows native job 118625552333 的 Shell coordination 组为 56 passed、2 failed，两个原有未监听 localhost 夹具在 5 秒 deadline 内未结算。

未跳过失败用例或放宽期限，独立 [test-only change](../2026-10-20-stabilize-coordination-rejection-fixtures/verification.md) 改为受控本地 HTTP 拒绝并断言恰好一个实际请求。旧官方 run 已 completed/cancelled，Release API 返回 404；新的候选仅在这些确认后以 expected tag-object lease 替换未发布的 v2.4.3。已公开 v2.4.2 不变，旧候选通过项不代替最终候选的正式门禁。

## 最终公开发布验证

最终发行源码为 `2069c43db5a3756bf65d1a275e8ed16bef3a86fa`，annotated tag object 为 `26155f3d58cf3b4c86dbf5e49e526b90910b50ff`。官方 [run 39425379616](https://github.com/LordCasser/grow/actions/runs/39425379616) 于 2026-10-20 13:57:29 UTC completed/success，20/20 jobs completed/success；没有失败、跳过或取消的必需 job，详见 [workflow-verification.json](workflow-verification.json)。该计数只含官方发布 run，不含为避免重复构建而取消的 push-triggered Core/native workflow。

Core 日志确认完整 Settings integration 231 passed、0 failed、0 ignored；新 show_model_provider 键盘、鼠标与注册集合检查实际通过。完整 Core library、CLI、3 项真实恢复/进程协议及 27 项选定 PTY 回归通过。Linux、macOS、Windows 原生协调与导出、Windows storage、OpenSpec 和十个平台发行构建全部通过。Windows 两个原失败夹具均 `... ok`，Shell coordination 58 passed、0 failed、0 ignored；详见独立夹具 change 验证。

[公开 v2.4.3](https://github.com/LordCasser/grow/releases/tag/v2.4.3) 发布于 2026-10-20 13:57:26 UTC，非 draft、非 prerelease，latest 指向 v2.4.3。公开资产集合恰好为十个平台 tar.gz 和 SHA256SUMS；十个清单摘要均与 GitHub 已上传资产摘要相等，远端 peeled tag 精确指向上述发行源码。实际下载 macOS ARM64 tar.gz 后本地 SHA256 一致，归档只有可执行 grow，`codesign --verify --strict` 通过。隔离 GROW_HOME 执行输出 `grow 2.4.3 (2069c43)`，没有安装或修改用户运行中的 Grow。详见 [asset-verification.json](asset-verification.json)。

发行 tag 固定在源码提交；发布证据另作文档提交推送 main，不重写已公开 tag。

发布证据写入后重新校验：current/all strict 16 passed、0 failed；archived strict 664 passed、0 failed；git diff --check 通过。
