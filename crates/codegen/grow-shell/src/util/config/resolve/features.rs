use crate::util::config::RemoteSettings;
use toml::Value as TomlValue;

/// Resolve whether ZDR users are allowed to use the product.
///
/// Precedence: requirements > env > config.toml > managed > remote settings > default (false).
pub fn resolve_zdr_access_enabled(
    requirements: Option<&TomlValue>,
    user: Option<&TomlValue>,
    managed: Option<&TomlValue>,
    remote: Option<&RemoteSettings>,
) -> bool {
    use crate::agent::config::BoolFlag;
    fn from_toml(v: Option<&TomlValue>) -> Option<bool> {
        v?.get("features")?.get("zdr_access_enabled")?.as_bool()
    }
    BoolFlag::env("GROW_ZDR_ACCESS_ENABLED")
        .requirement(from_toml(requirements))
        .config(from_toml(user))
        .managed(from_toml(managed))
        .feature_flag(remote.and_then(|r| r.zdr_access_enabled))
        .resolve()
        .value
}
