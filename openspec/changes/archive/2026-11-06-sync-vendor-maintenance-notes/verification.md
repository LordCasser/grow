# Verification

2026-11-06，HEAD8ab486a6，文档Tier0。父读取Cargo.lock中pprof0.15→nix0.26.4、portable-pty0.9→nix0.28.0、mac_address1.1.8→nix0.29.0以及registry nix0.31.3；读取根patch和workspace nix^0.31、sqlite-vec/nix-ohos Cargo维护头、nix-ohos MIT许可证、third_party索引/NOTICE及Core native harness命令。

实际更新仅third_party/README.md、NOTICE和docs/architecture/dependency-workarounds.md：补nix索引、实际resolvedgraph和已维护sqlite源修复，明确移除前逐项等价性与native strict/sanitizer门槛。开发者说明中原sqlite语义说明已存在，未将它记为本次新增发现；本次补测试入口及覆盖上限，删除易过期的pprof/两个nix消费者‘latest’声明。未修改vendor源码/依赖版本、未升级或删除patch、未运行本轮未改变的C测试。原报告的历史事实保留。

已运行单项strict和git diff --check通过，父校对实际diff与所有新增相对路径/许可证。`openspec archive sync-vendor-maintenance-notes --yes`成功；归档后全量strict19/19、archived873/873通过，日志为`/tmp/grow-backlog-vendor-{specs,archives}.log`。其他在途coder的业务代码与验证不属于本文档项完成证据。backlog中该文档待办已移出，完整原发现和本次证据仍保留；其他vendor或依赖用途待核实项不因此关闭。
