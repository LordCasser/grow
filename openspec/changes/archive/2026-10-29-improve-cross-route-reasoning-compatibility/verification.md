# Verification

## 范围与结论

第一阶段补齐两条既有路径：从完整结构化 HTTP/SSE 错误保留 typed reasoning 要求；已确认要求的 Responses 路由回放普通 assistant 回答前已有的可见 reasoning。未增加 provider registry、配置开关、持久化格式或额外重试预算。

真实验证分开保存，不覆盖失败：首轮默认配置 5/9 阶段通过；补跑矩阵 9/9，通过三个 API 格式的六个切换方向，同 provider 换 model、同协议跨 provider 与切回旧 route。两轮的普通回答续接均通过。最终 SSE 诊断字段修复后的重新构建及矩阵结果见下文最终复验。

这几组真实端点没有返回缺 reasoning 的 400，portable_reasoning_backend 始终为 null。真实证据证明的是切换后的请求可被所测端点接受、历史事实续接及工具不重复执行；不能据此声称已在真实端点验证“400 → 学习回放 → 恢复成功”。该分支由完整错误、DTO 往返、ChatState ACK、wire 投影及重复拒绝的定向回归覆盖。普通回答 reasoning 的真实强制回放接受性仍未验证。

## 构建与回归

Rust/Cargo 1.93.1；PATH=/Users/lordcasser/.cargo/bin:$PATH、CARGO_INCREMENTAL=0、CARGO_BUILD_JOBS=6、CARGO_TARGET_DIR=/private/tmp/grow-route-compat-target。Shell 测试另设 RUST_MIN_STACK=16777216。

| 验证 | 命令关键部分 | 结果 |
| --- | --- | --- |
| ChatState | cargo test --locked -p sampling-types -p sampler -p chat-state --lib | ChatState 602 passed / 3 ignored |
| SamplingTypes / Sampler，最终 SSE 修复后 | cargo test --locked -p sampling-types -p sampler --lib -- --test-threads=1 | 319 / 259 passed |
| Session 采样恢复 | cargo test --locked -p shell --lib session::actor::turn::sampling -- --test-threads=4 | 11 passed |
| Session 模型切换 | cargo test --locked -p shell --lib session::actor::model_switch::tests -- --test-threads=4 | 31 passed |
| CLI 构建 | cargo build --locked -p cli --bin grow --no-default-features --features sandbox-enforce | 成功，系统 allocator，保留 sandbox |
| 格式与差异 | cargo fmt --all -- --check；git diff --check | 通过 |

以上去重后为 1222 passed、3 ignored，不是全仓测试。构建和相关源码哈希记录于 validation-manifest.json。

开发期间修复过 &str.as_str、DTO 重建调用、param 类型等编译问题，否定冲突反例也先失败后修复。并行核心测试一轮在既有 sampling_auth_logs_omit_credentials 的 logs.contains("client_post") 断言失败；日志事件及测试均不在本次改动内，串行整包两次通过。并行敏感原因未定位，没有把失败删掉或算作全绿。

最后复核发现 named SSE 把 request_id 拼进 message 后会干扰精确分类。现已使用原始 message 分类，诊断 ID 只参与展示；新反例让 ID 含另一 backend 的字段名，仍正确识别原始 param。非字符串 param 保留 API 错误正文，但不启用 reasoning 恢复。

## 真实验证方法

live_validation.py 通过实际 grow agent stdio / ACP 在隔离 Grow home 与工作目录运行，initialize → authenticate → session/new → session/set_config_option → session/prompt。每轮矩阵在同一进程、同一 resident session 完成，不重建或丢弃历史。另建同进程会话验证普通回答续接。

每个阶段要求执行一次 python3 record_step.py N，脚本以系统随机数生成 token 并落盘。模型应输出当前 token 和历史中前一次成功执行的 token；后者未写入新 prompt。以 ACP tool_call ID 对应的 completed 更新及工具输出中的 token、下一次实际请求中的工具配对、落盘执行计数交叉核对，不能只凭最终回答通过。普通回答依次计算 6056 和 18175，两轮均没有工具调用。

extract_live_evidence.py 从 canonical Timeline 的 sampling_evidence 请求/响应及原始 ACP 提取数据，排除标题等 sideband 调用。只保存业务检查、受限夹具命令、状态、计数和哈希；不保存 reasoning 正文、完整 provider body、完整错误诊断或 endpoint 路径。源配置只复制所选模型和认证到 0700 临时目录内的 0600 文件，未改用户配置或安装二进制。补跑仅在临时配置中为已有 high 选项的模型选择 high，不给没有该选项的模型伪造能力。

## 首轮默认配置：primary-live-evidence.json

| 阶段 | 目标 route | 结果 |
| --- | --- | --- |
| 1 | opencode/glm-5.3-flash，Chat | 通过 |
| 2 | opencode/deepseek-v4.1-flash，Chat | 通过 |
| 3 | neorouter/deepseek-flash，Responses | 通过 |
| 4 | qwen2/qwen3.8-flash，Messages | HTTP 403，未执行工具 |
| 5 | opencode/deepseek-v4.1-flash，Chat | 模型补做阶段 4 再做 5，两次调用，严格判失败 |
| 6 | qwen2/qwen3.8-flash，Messages | HTTP 403，未执行工具 |
| 7 | neorouter/deepseek-flash，Responses | 模型补做阶段 6 再做 7，两次调用，严格判失败 |
| 8 | opencode/deepseek-v4.1-flash，Chat | 通过 |
| 9 | bigmodel/glm-5.3-flash，Chat | 通过 |

共 20 个主请求：18 次 HTTP 200、2 次 HTTP 403；403 均 attempt 1 终止，未变成 reasoning 恢复。最终每个夹具阶段虽恰好执行一次，但阶段 5/7 违反本 prompt 的单次调用约束，所以不能报 9/9。其原因是服务失败后仍保留了未完成用户请求，后续模型继续处理；不是历史工具已执行结果被重新触发。Messages 的实际生成续接在首轮未验证。

首轮 driver 在全部 RPC 完成后的 close 因 reader 属性拼写错误崩溃，未生成最终 summary；从完整原始 RPC、Timeline 和执行记录重建这份证据。已修复 driver 并对 cleanup、summary 落盘做本地 transport smoke；smoke 不计入真实模型证据。

## 补跑：direct-live-evidence.json

| 阶段 | 目标 route | 切换覆盖 | 结果 |
| --- | --- | --- | --- |
| 1 | opencode/glm-5.3-flash | 初始 Chat | 通过 |
| 2 | opencode/deepseek-v4.1-flash | 同 provider 换 model | 通过 |
| 3 | deepseek/deepseek-flash | Chat → Responses | 通过 |
| 4 | opencode/qwen3.8-max | Responses → Messages | 通过 |
| 5 | opencode/deepseek-v4.1-flash | Messages → Chat | 通过 |
| 6 | opencode/qwen3.8-max | Chat → Messages | 通过 |
| 7 | deepseek/deepseek-flash | Messages → Responses | 通过 |
| 8 | opencode/deepseek-v4.1-flash | Responses → Chat | 通过 |
| 9 | bigmodel/glm-5.3-flash | Chat 跨 provider/model | 通过 |

deepseek/deepseek-flash 实际 wire model 是配置映射的 deepseek-v4-flash。Chat/Responses 实际请求有 high；Messages 未额外指定 effort。矩阵 18 个请求及普通回答 2 个请求全部 HTTP 200，均 attempt 1。每个切换后的首次请求 native_span_count=0、portable_reasoning_backend=null、无 reasoning opaque 字段；所有请求的工具调用/结果 ID 配对各一次。相同路由后续请求可使用本路由新产生的 native span，切回旧 route 没有复活旧 span。

## 最终复验与归档

最终 SSE 修复后的本地二进制复验见 final-live-evidence.json：同一 resident session 中 9/9 阶段通过，另一个同进程会话的两轮普通回答通过。20 个主请求均 HTTP 200、attempt 1；每个切换后首请求 native span 为 0，所有请求 tool ID 配对完整，夹具阶段 1–9 各完成一次。portable_reasoning_backend 仍全为 null，未触发 400 恢复的限制不变。

validation-manifest.json 分别保留前两轮和最终构建的 binary/source 哈希，最终源码与复验二进制一致。三个证据 JSON 经过源配置凭据比对扫描，未发现凭据；只保留此次隔离运行的随机身份、公开端点 host、模型名及统计。三个私有运行目录（含临时认证及原始日志）均已删除；cargo clean 清理 81,728 个文件、10.5 GiB 构建产物；Python 缓存已清理。未留下凭据文件。

OpenSpec：归档前 strict 17/17；已归档并合入 model-sampling 的两个 requirement；归档后全量 strict 16/16、archive 745/745 通过。第一次 archive 检查因最后“归档/清理”任务尚未勾选而报 7/8，清理完成并核对后标为 8/8，再验通过。git diff --check 与最终 cargo fmt 检查均通过。
