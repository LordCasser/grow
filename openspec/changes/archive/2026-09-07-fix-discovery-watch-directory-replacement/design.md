## Evidence
ProjectDiscoveryWatcher::start 在.grow存在时 initial=.grow，不保留project_root watch。attach_new_refresh_dirs 路径已在 refreshed_dirs 则跳过，即使旧inode已删除。Skills watch plan 仅缺失根才观察parent，存在类似风险。

## Constraints
保留非递归父目录，只递归明确skills根；需要当前目录实体身份而不是mtime（普通写入会改mtime）。不可将持续重挂递归目录作为默认处理。复用现有跨平台文件identity能力前先核对可用API，避免平台专用临时方案。

## Validation
真实临时project，初始.grow存在，删除后重建，再次修改skills/workflows均应被观察；同时检查没有替换时刷新不重复挂载。

## 当前验证记录
macOS 实际 OS 测试 project_discovery_observes_recreated_root 在未修改 production 逻辑下通过（1 test，6.42s），因此尚不能声称该平台的删除重建故障已复现。新增 refresh_detects_atomic_directory_replacement 检查目录替换后的显式注册；编译阶段因 No space left on device 失败，未执行测试。磁盘仅余 118 MiB。生产修复尚未实施，保留待验证状态。

## Implementation decision
使用锁文件中已存在的 same-file 1.0.6，显式声明 Shell 依赖；缓存 PathBuf -> Handle，保留少量根目录句柄以避免旧实体 inode 被复用。对比当前 handle；相同实体不重新 watch，删除或替换则移除旧注册，成功挂载后才缓存新 handle。项目父目录始终非递归监听；Grow root 也纳入刷新目标。独立 harness include 实际 watcher.rs，只有未被这些测试使用的配置发现入口使用不可调用 stub，project_root 使用临时根。旧实现原子替换测试确实失败（replacement needs a new watch）。
