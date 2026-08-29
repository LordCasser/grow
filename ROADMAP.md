# ROADMAP

## Managed Configuration：默认关闭与远程同步服务

这件事分成两层：`managed_config.toml` 的客户端加载/合并链路已经存在，但当前没有配套的配置管理服务端。个人使用场景不应该因为这套预留能力而默认进入远程同步路径。

### 1. 默认关闭自动同步

- [ ] 将 `managed_config` 的默认值从开启调整为关闭。
- [ ] 保留显式配置和 `grow setup` 的主动触发能力；用户或组织明确配置 deployment key、端点后，仍可以启用同步。
- [ ] 明确区分“自动同步开关”和“本地 managed 配置加载”：关闭自动同步不应破坏 `/etc/grow/managed_config.toml`、已有缓存或测试注入的配置层。
- [ ] 补齐 `config.example.toml`、README 和用户文档，说明 `GROW_MANAGED_CONFIG`、`GROW_MANAGED_CONFIG_URL`、`GROW_DEPLOYMENT_KEY` 的作用和优先级。
- [ ] 验收：没有 deployment key 时启动不发起配置网络请求；没有显式启用时不启动无意义的后台同步行为；`grow setup` 仍可作为主动操作使用。

### 2. 适当时建设自动配置同步服务器

- [ ] 建设独立的 deployment-config 服务，为 Grow 客户端提供配置拉取、版本管理和撤回能力。
- [ ] 实现客户端当前约定的接口：`GET /deployment/config`，使用 `Authorization: Bearer <deployment-key>` 鉴权，返回 `deployment_id`、`managed_config`、`requirements` 及签名信封。
- [ ] 配置服务维护 deployment key 到 deployment、团队和配置版本的映射，支持密钥轮换、吊销、过期和审计。
- [ ] 保持 `managed_config.toml` 与 `requirements.toml` 的职责边界：前者提供组织级默认配置，后者承载需要高于用户配置的约束。
- [ ] 完成 Ed25519 签名发行链路。签名私钥只存在于服务端；客户端信任的公钥需要通过版本化构建或明确的密钥轮换机制发布。
- [ ] 明确首次引导方式：远程端点和 deployment key 必须在第一次 `grow setup` 前由本地配置、环境变量或设备管理系统提供，不能依赖尚未拉取的 managed 配置自身。
- [ ] 增加服务端与客户端的端到端测试，覆盖空配置、配置撤回、配置版本切换、密钥轮换、签名失败、网络失败和 fail-closed 策略。
- [ ] 在有明确团队管理需求、服务端部署边界和密钥运营方案之前，不把该服务端作为 Grow 的默认依赖。
