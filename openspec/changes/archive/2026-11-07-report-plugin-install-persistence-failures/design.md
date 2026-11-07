# Design

## 证据与所有者

`plugin.rs::install_plugin`解析source、load registry、install_from_source发布文件、insert/build_installed_repo后调用save_registry_or_warn，随后post_install_plugin重新读durable registry以auto-enable。actor hooks_plugins.rs::Install复制同样链并warn保存失败。CLI `pager/plugin_cmd.rs::cmd_install`已经消费共享Result，Err时log failure并bail；public `plugin_cmd::run`可验证这一真实dispatch，不需要每次低层修改都重建整个grow再测函数替身。

现`InstallRegistry::try_load_from(InstallRegistry::resolve_install_dir())`只把NotFound当空，坏读取/JSON返回InstallError；save/save_atomic已有Result与既有after-serialize故障开关。无需新增public loader/helper/fault框架或修改只读list容错。普通install在save前已发布artifact，区别于marketplace staging事务，不能凭另一个owner有rollback推断这里也能原子回滚。

## 冻结合同

共享install application按序：fallible load成功→原install source/发布→构造并插入结果→save成功→原post-install→InstallOutcome。load失败必须在clone/copy/publish和config编辑之前退出，原registry字节/已有repo保持。save失败必须返回Err，并包含本次已知repo key/path、保存原因和已发布但未完整登记的事实；不继续post_install、不发成功diagnostic、不请求reload。保留artifact以供诊断/重试判断，不未经需要删除它，也不新建一般rollback协议。

使用现InstallError可以承载失败（保存后的部分完成可用既有InstallFailed detail加context），不为无人消费的阶段码新建public错误族；不得把Err塞入warnings后返回Ok。CLI已有错误传播直接保留，若测试揭示public run吞Err则该同一安装结果的消费者适配随此修正；其trust gate保持。

actor Install只验证原空source输入，调用共享`install_plugin(source,cwd).await`，成功按现names/warnings/is_local结果格式化，失败映射现ActionOutcome InternalError、requires_reload=false/requires_restart=false，消息明确部分完成。不要保第二份registry load/save或低层install调用。原post-install config auto-enable错误继续作为显式warnings，不把本次扩成配置多字段原子事务；只有核心registry成功后才能走到这一步。

范围为普通Git/local安装，不改CLI marketplace-ref install、marketplace事务、Update/Uninstall、require-sha、source选择/信任、plugin identity或配置TOML writer。AC-07更新采用在另change，AC-08余下两个operation仍开放。

## Coder Task Spec

唯一功能为CLI/ACP普通安装不得在registry load/save失败时声称成功。write共享plugin.rs Install函数/必要注释及actor Install映射；同一行为必要CLI公共run测试或小的调用方适配允许，勿顺带修改卸载/更新枚举、删除所有save_registry_or_warn（其它尚在用）或重写InstallRegistry。write必须等现Rust/Cargo writer释放，父保design/spec/tasks/docs/backlog/archive。

先用隔离本地合法plugin源＋已有旧registry＋既有save失败开关，真实共享application取得有效旧红（旧行为Ok/成功但新entry缺失）；随后最小修复、新绿，再补真实ACP Install和CLI public plugin_cmd::run(trust=true)错误传播。使用真实SaveResult hook或文件rename失败，不靠当前用户权限下不一定生效的chmod。badregistry字节控制在文件发布前明确Err，旧字节不变、没有新repo artifact/configenable。save失败控制保已发布artifact和旧记录字节，错误不伪称rollback。解除故障后的正常独立安装以新source或可诊断路径验证，不能无视AlreadyInstalled而伪造自动重试政策。

保成功安装name/path/record一致、原warnings显示、成功reload提示；post_install未运行可由隔离config无新增enable及outcome证明，不另建production计数器。使用隔离HOME/GROW_HOME/project并考虑进程OnceLock；所有phase可在同一隔离test里，保存fault必须恢复且不作用真实用户配置。低层原marketplace回滚测试不因本项复用到新的所有者或计为普通安装原子性证据。

## Review Plan

Tier 3因安装登记数据完整性：父审实际common owner、load/save失败停止点、真实ACP/CLI dispatch与artifact/registry/config对照，必要失败control不能只证明helper返回Err。库/近邻回归按受影响行为执行，不全workspace或重复已绿套件；最终集成版本再验CLI产物层的正常/失败出口。未跑Windows/外部网络现场照实记录。同步开发说明与AC-08安装子项状态，不把整个AC-08标完；strict/archive和后置all/archived后闭合此change。
