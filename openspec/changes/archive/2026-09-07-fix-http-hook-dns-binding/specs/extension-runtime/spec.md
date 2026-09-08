## ADDED Requirements

### Requirement: HTTP hooks connect to validated addresses
HTTP Hook SHALL 使用 URL 校验阶段取得且全部通过 IP 检查的地址集合建立直连，不在请求时重新解析目标或自动采用系统代理。原 URL 主机名 SHALL 用于 Host 与 TLS 身份校验，跳转仍禁止。

#### Scenario: 校验后域名解析变化
- **WHEN** 地址已校验而域名后续解析变化
- **THEN** 请求仍使用已校验地址集合，不通过后续解析选择新地址。

#### Scenario: IPv6 字面地址
- **WHEN** URL 主机是 IPv6 字面地址
- **THEN** 直接按 IP 分类校验，不执行 DNS 解析。
