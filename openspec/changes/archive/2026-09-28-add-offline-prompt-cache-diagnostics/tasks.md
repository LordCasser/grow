## 1. 证据读取与对比

- [x] 1.1 用合成 Timeline/chunk fixtures 核对现有 evidence 格式和 attempt 身份，完成有界只读加载；缺块、digest 损坏和逃逸引用的测试必须失败且不改输入文件。
- [x] 1.2 实现显式 baseline/request 选择与可比较性判定；用重试、并发 Sideband、跨 branch/route 及缺少完整 route 证据的 fixtures 验证归属。
- [x] 1.3 实现三协议 tools/system/history/settings/cache hints/source projection 差异摘要；验证 tools 重排不会被排序消掉，也不会输出伪造的 token 公共前缀或远端 miss 原因。
- [x] 1.4 接入可取得的原始 usage 与归一化计量，在 `preserve-cache-usage-availability` 完成后交叉核对字段 presence、覆盖口径和不可用状态。

## 2. 验证与交付

- [x] 2.1 运行 `python3 -m unittest discover -s scripts -p 'test_analyze_prompt_cache.py'`；验证 JSON/text 输出确定、默认不含正文或大载荷、仅保留有界请求对。
- [x] 2.2 对一个包含 sampling artifacts 的完整快照执行只读 smoke，核对 identity 与文件 digest；把命令、结果或未执行原因写入 `verification.md`。
- [x] 2.3 在 `docs/development.md` 说明工具入口、snapshot 要求和证据限制，确认无生产行为改动；通过 `openspec validate --all --strict --no-interactive`。
- [x] 2.4 完成工具验证后以 `--skip-specs` 归档，并执行全量规范及 `openspec validate --archived --no-interactive`；不得将工具测试标成 provider 性能验证。
