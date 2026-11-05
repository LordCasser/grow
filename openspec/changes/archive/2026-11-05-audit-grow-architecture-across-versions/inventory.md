# 审计范围与基线

当前基线 HEAD：`d6128138c`（2026-11-04）。当前根版本为 2.6.0；发布标签 v2.6.0 指向 `5475dff997f0`，不能把 HEAD 当作该标签的精确源码。Git 非 shallow。以下清单来自 `cargo metadata --locked --no-deps --format-version 1`、`git tag --list --sort=version:refname`、各 tag 的 `git ls-tree -r`、`git rev-parse <tag>^{commit}` 和根 Cargo.toml。

开始时已有用户改动，包含 Agent/repo/config discovery、trust/Git 路径、Task effort 暴露与测试、相关规范和文档；全部保留。同轮 `allow-session-model-effort-control` 修改的区域在该 change 完成后补最终复核。本清单表示审查责任已覆盖，不表示各区已经通过或没有缺陷。

## 当前模块责任表

Cargo metadata 含 57 个 workspace package，以下每个 package 只分配一个主要报告；跨区接口由父架构 owner 复核。

| 分区/报告 | Workspace packages | 额外执行链与目录 |
| --- | --- | --- |
| [Session runtime](reviews/session-runtime.md) | shell, chat-state, prompt-queue, workflow | shell agent/session 的接纳、控制、Goal/Plan/Workflow、Timeline、恢复与生命周期；sampling、tool authorization、config等专门路径分别与对应区交叉核对 |
| [Sampling](reviews/sampling.md) | sampler, sampling-types, compaction, token-estimation, grow-http, auth | shell sampling/compaction 消费接口 |
| [Tools/authorization](reviews/tools-authorization-recovery.md) | tools, tool-protocol, tool-runtime, tool-types, sandbox, hooks, mcp, workspace-types | workspace permission、shell tool preflight/dispatch 消费接口 |
| [Agent/config](reviews/agent-config.md) | agent, config, config-types, plugin-marketplace, extension-types, announcements | shell config/extensions、prompt/规则/技能与工具集变体、模型目录发布 |
| [Workspace/storage](reviews/workspace-storage.md) | workspace, fast-worktree, grow-gix-status, hunk-tracker, codebase-graph, fsnotify, memory, sqlite-journal, paths | workspace permission 归 tools/authorization；其余 trust/session/Git/FS/index 全含 |
| [Client/UI](reviews/client-ui.md) | pager, pager-minimal, pager-render, markdown, mermaid, ratatui-inline, ratatui-textarea, client-support, acp-transport, shell-base | 输入/异步绑定/展示投影/恢复/终端所有权 |
| [Product/build](reviews/product-build.md) | cli, update, version, diagnostics, crash-handler, tracing-macros, tty-utils, ptyctl, ptyctl-cli, pager-pty-harness, test-support, test-utils, dagre_rust, mermaid-to-svg | scripts、.github/workflows、bin、cp、根配置/文档、third_party/{nono,nix-ohos,sqlite-vec,dagre_rust,mermaid-to-svg}、markdown/fuzz、平台构建与依赖 workaround |

第三方 vendored patch 及上游代码区别审查：当前必要的维护差异、调用契约和保留理由均在范围；不声称穷举所有上游算法正确性。Cargo dev/build/target 依赖分别识别，不能以开发依赖的存在推断生产层循环。历史移除模块不因不在当前57个package而排除，逐版本分区表追踪旧路径。

## 发布与旁支核对

只读 `git ls-remote --tags --refs origin` 确认远端和本地均为46个标签，remote-only/local-only均为空；本次没有 fetch、checkout、重写历史或外部发布。

| Ref | Commit | 相对 main 的 main-only / ref-only | 审查处理 |
| --- | --- | --- | --- |
| main, origin/main | `d6128138c` | 0 / 0 | 当前代码基线 |
| codex/mcp-policy-sdk-identity | `6de47919c` | 80 / 2 | 两个独有提交 `5b375b07c`、`6de47919c`，交 tools/authorization 核对未合入 MCP policy/SDK identity，不当作当前已交付能力 |
| origin/codex/pre-release-v2.1.10-local | `5ef50112d` | 138 / 2 | 两个独有提交 `ae1f406a8`、`5ef50112d`，交 sampling/session区核对 sideband工具证据与发布身份差异 |
| upstream/main | `bc7f02edd` | 678 / 27 | 上游同步历史，构建区核对 fork/导入边界与遗留模块，不将其当作当前Grow发布标签 |

当前 refs 中的 `origin` 和 `upstream` 是对应默认分支的符号引用。`git log --all` 还可看到stash/snapshot对象，它们不是正式版本或当前工作区；保留原对象，不以审计为由清理。

## 历史模块并集

逐 tag 解析所有 Cargo.toml 的 `[package].name` 并按 blob OID 复用解析，46个标签共出现126个包名。其中69个名字不在当前57个workspace package名单中；多数是v1.0.0的grow-/xai-改名，另含当前非workspace的vendor/fuzz，不能把69解释为删除了69种功能。

除直接前缀迁移外，下列历史独立模块已明确分配有界审查职责与合并/移除去向核对：

| 历史模块（v1.0.0） | 已读取的职责/审查分区 |
| --- | --- |
| grow-subagent-resolution, grow-shell-session-support, xai-agent-lifecycle, xai-interjection-core | subagent解析、managed MCP、host生命周期contributor、插话核心；Session runtime |
| grow-tools-api | protobuf DTO、config validation、slash wording与工具命名；Tools/authorization |
| grow-shared, grow-markdown-core | 客户端公共clipboard/session/ui_config与markdown分层；Client/UI |
| xai-hooks-plugins-types | hooks/plugins ACP DTO边界；Agent/config |
| xai-file-utils | streaming SHA256、workspace classifier；Workspace/storage |
| grow-pager-bin, xai-proto-build, xai-system-power, prod-mc-cli-chat-proxy-types | 产品/协议生成/平台/旧服务入口；Product/build |
| graphlib_rust, ordered_hashmap | 历史vendor算法容器与依赖必要性；Product/build，Client消费关系交叉核对 |

当前非workspace包 `markdown-fuzz`、`nono`、`nix`、`sqlite-vec` 及其patch与工具用途在Product/build范围。历史包名称枚举来自实际Git blob，不是根据当前目录名猜测；各分区仍需给出对应结构变化证据。

## 全部发布标签

Cargo manifests 为 Git tree 中所有 Cargo.toml 的数量，包含 vendor/fuzz，与 workspace package 数不是同一口径。逐标签结构与架构变更的审视见各分区版本表；下表是全局完整版本身份。

| Tag | Commit | Date | Root version | Cargo manifests |
| --- | --- | --- | --- | --- |
| v1.0.0 | `da034e22d87b` | 2026-07-31 | 1.0.0 | 71 |
| v1.1.0 | `7ea59a9db583` | 2026-08-02 | 1.1.0 | 61 |
| v1.1.1 | `4f8e0232eae0` | 2026-08-02 | 1.1.1 | 62 |
| v1.1.2 | `abca23e0b295` | 2026-08-03 | 1.1.2 | 62 |
| v1.1.3 | `1b24b9f03668` | 2026-08-03 | 1.1.3 | 62 |
| v1.1.4 | `28b798dc17b2` | 2026-08-05 | 1.1.4 | 62 |
| v1.1.5 | `05a3fbcc57c1` | 2026-08-09 | 1.1.5 | 62 |
| v1.1.6 | `ca3400250af9` | 2026-08-11 | 1.1.6 | 62 |
| v1.1.7 | `62d9b47fd9d5` | 2026-08-13 | 1.1.7 | 62 |
| v1.1.8 | `bde0050632c2` | 2026-08-14 | 1.1.8 | 62 |
| v1.1.9 | `465d9b12a42d` | 2026-08-14 | 1.1.9 | 62 |
| v1.1.10 | `38b2d7fc7039` | 2026-08-15 | 1.1.10 | 62 |
| v1.1.11 | `79495bcd40e5` | 2026-08-16 | 1.1.11 | 62 |
| v2.0.0 | `2282940c1120` | 2026-08-26 | 2.0.0 | 62 |
| v2.0.1 | `ae8df2a3b4b9` | 2026-08-27 | 2.0.1 | 62 |
| v2.0.2 | `7233f4e51e6d` | 2026-08-29 | 2.0.2 | 62 |
| v2.0.3 | `5aabf367b53a` | 2026-08-29 | 2.0.3 | 62 |
| v2.0.4 | `1b52597ccdb5` | 2026-08-30 | 2.0.4 | 62 |
| v2.1.0 | `d43756888adf` | 2026-09-01 | 2.1.0 | 62 |
| v2.1.1 | `96565d6975d8` | 2026-09-01 | 2.1.1 | 62 |
| v2.1.2 | `1ca5c6b120ea` | 2026-09-03 | 2.1.2 | 62 |
| v2.1.3 | `09c68b808c41` | 2026-09-03 | 2.1.3 | 62 |
| v2.1.4 | `87d1df4c3b0a` | 2026-09-05 | 2.1.4 | 62 |
| v2.1.5 | `a8dc93716f9a` | 2026-09-08 | 2.1.5 | 62 |
| v2.1.6 | `2a8297e818b6` | 2026-09-11 | 2.1.6 | 62 |
| v2.1.7 | `9f721d8e927c` | 2026-09-14 | 2.1.7 | 62 |
| v2.1.8 | `7ed6441a8fb8` | 2026-09-15 | 2.1.8 | 62 |
| v2.1.9 | `bd1f5085a9c9` | 2026-09-16 | 2.1.9 | 62 |
| v2.1.10 | `c17b085d07f8` | 2026-09-17 | 2.1.10 | 62 |
| v2.1.11 | `3024dad1ef75` | 2026-09-17 | 2.1.11 | 62 |
| v2.2.0 | `a84ce03b0d13` | 2026-09-25 | 2.2.0 | 62 |
| v2.2.1 | `b38627016b41` | 2026-09-28 | 2.2.1 | 62 |
| v2.2.2 | `afbe3e48209d` | 2026-09-28 | 2.2.2 | 62 |
| v2.3.0 | `ef665ec41eac` | 2026-09-30 | 2.3.0 | 62 |
| v2.3.1 | `7b7d56ab98a6` | 2026-10-03 | 2.3.1 | 62 |
| v2.4.0 | `cd5d88ec7f5d` | 2026-10-19 | 2.4.0 | 62 |
| v2.4.1 | `5fcbd12b0c10` | 2026-10-20 | 2.4.1 | 62 |
| v2.4.2 | `ecb0f9cc6b48` | 2026-10-20 | 2.4.2 | 62 |
| v2.4.3 | `2069c43db5a3` | 2026-10-20 | 2.4.3 | 62 |
| v2.5.0 | `8443a4d1648d` | 2026-10-22 | 2.5.0 | 62 |
| v2.5.1 | `a58b5dd7e8c7` | 2026-10-25 | 2.5.1 | 62 |
| v2.5.2 | `1c88fccb4adf` | 2026-10-29 | 2.5.2 | 62 |
| v2.5.3 | `fce2f3d590dd` | 2026-10-29 | 2.5.3 | 62 |
| v2.5.4 | `069f712894ca` | 2026-10-29 | 2.5.4 | 62 |
| v2.5.5 | `599873595c94` | 2026-10-31 | 2.5.5 | 62 |
| v2.6.0 | `5475dff997f0` | 2026-11-04 | 2.6.0 | 62 |

初次生成矩阵时系统 `python3` 缺少 `tomllib`，命令未产生版本数据；改用不依赖 TOML 库的 Git 对象分析后逐项成功，根版本来自 manifest 的 version 声明。审计不因此安装额外依赖。
