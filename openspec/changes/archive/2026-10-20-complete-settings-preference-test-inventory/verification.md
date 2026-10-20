# 验证

## 已核对范围

保留原四个失败断言，补齐 show_model_provider 的 ALL_SETTINGS_EXERCISED、Bool membership、default expected、typed setter。补键盘 Space、鼠标两阶段选择/切换，并显式复位线程缓存；完整注册表断言不跳过缺项。只读对抗性复查未发现该补丁的阻塞问题。

完整审计见 [audit.md](audit.md)：42 个 catalog key 与 integration 登记精确相等；读取、提交、reset、persist、rollback 逐条核对未发现其他确认漏接。明确配置专用/延期/独立入口，以及“清单完整不代表全部交互已验证”的覆盖限制。该限制保留为 backlog。

## 本地执行

在 workspace/本地包 lock 已更新为 2.4.3 的工作树运行：

`cargo test --locked -p pager -p shell --lib --test settings_e2e --no-run --message-format=json`

构建完成 exit 0，4m14s；从 Cargo compiler-artifact 读取三个唯一的 executable（profile.test=true，pager lib、shell lib、settings_e2e），后续直接执行对应产物，并以其 manifest 所在 package 为 cwd，避免再次 Cargo 枚举引发重复编译。

| 产物与筛选 | 结果 |
| --- | --- |
| settings_e2e，全组，--test-threads=4 | 231 passed，0 failed，0 ignored |
| pager lib，settings，--test-threads=4 | 331 passed，0 failed，1 ignored |
| pager lib，subagent_permission，--test-threads=4 | 20 passed，0 failed，0 ignored |
| shell lib，subagent_permission，--test-threads=4 | 5 passed，0 failed，0 ignored |

上述组有交集，不累加为独立测试总数；ignored 不计作通过。此前四个失败 every_registered_setting_is_exercised、registry_kind_membership_through_pr_14、defaults_round_trip_through_registry、settings_value_payload_matches_kind 在完整 Settings integration 中均通过。键盘/鼠标与子 Agent 三模式、搜索用例均实际执行。

`python3 -m unittest discover -s scripts/tests`：Ran 34，33 passed、1 skipped，exit 0。指定修改 Rust 文件 rustfmt --check、git diff --check 通过。完整 Settings integration 已加入 Core reusable workflow；release 仍将 validated immutable SHA 传给 Core，checkout 后核对同一 SHA。实际托管门禁结果由 v2.4.3 发布记录补充。

配置仅使用临时测试目录，未修改实际用户权限或安装二进制。

归档后严格校验：Totals: 16 passed, 0 failed (16 items)；Totals: 663 passed, 0 failed (663 items)。
