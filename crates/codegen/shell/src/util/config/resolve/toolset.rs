use toml::Value as TomlValue;
use tools::implementations::grow_build::ask_user_question;

/// Resolve the local search-tool shadow switches. The explicit environment
/// kill switch wins, followed by the per-tool environment variable, local
/// effective config, and the default-on value.
pub fn resolve_search_tools_enabled(user: Option<&TomlValue>) -> (bool, bool) {
    let disable = config::env_bool("DISABLE_EMBEDDED_SEARCH_TOOLS");
    fn from_toml(v: Option<&TomlValue>, key: &str) -> Option<bool> {
        v?.get("toolset")?.get("bash")?.get(key)?.as_bool()
    }
    let resolve = |primary: &str, alias: &str, key: &str| -> bool {
        let env = config::env_bool(primary).or_else(|| config::env_bool(alias));
        resolve_search_tool_enabled(disable, env, from_toml(user, key))
    };
    (
        resolve("GROW_TOOLS_FIND_BFS", "GROW_FIND_BFS", "find_bfs"),
        resolve("GROW_TOOLS_GREP_UGREP", "GROW_GREP_UGREP", "grep_ugrep"),
    )
}

pub fn resolve_shell_env_policy(
    effective_cfg: Option<&TomlValue>,
) -> Option<tools::util::ShellEnvironmentPolicy> {
    let value = effective_cfg?.get("shell_environment_policy")?.clone();
    match value.try_into::<tools::util::ShellEnvironmentPolicy>() {
        Ok(policy) => Some(policy),
        Err(error) => {
            tracing::warn!(
                %error,
                "failed to parse [shell_environment_policy]; inheriting the full environment"
            );
            None
        }
    }
}

fn resolve_search_tool_enabled(
    disable: Option<bool>,
    env: Option<bool>,
    config: Option<bool>,
) -> bool {
    if disable == Some(true) {
        return false;
    }
    env.or(config).unwrap_or(true)
}

const ENV_LOGIN_SHELL_CAPTURE: &str = "GROW_LOGIN_ENV";

fn login_shell_capture_from_toml(v: Option<&TomlValue>) -> Option<bool> {
    v?.get("toolset")?
        .get("bash")?
        .get("login_shell_capture")?
        .as_bool()
}

pub fn resolve_login_shell_capture(config: Option<&TomlValue>) -> bool {
    use crate::agent::config::BoolFlag;
    BoolFlag::env(ENV_LOGIN_SHELL_CAPTURE)
        .config(login_shell_capture_from_toml(config))
        .default(true)
        .resolve()
        .value
}

const ENV_ASK_USER_QUESTION_TIMEOUT_ENABLED: &str = "GROW_ASK_USER_QUESTION_TIMEOUT_ENABLED";
const ENV_ASK_USER_QUESTION_TIMEOUT_SECS: &str = "GROW_ASK_USER_QUESTION_TIMEOUT_SECS";

fn ask_user_question_timeout_enabled_from_toml(v: Option<&TomlValue>) -> Option<bool> {
    v?.get("toolset")?
        .get("ask_user_question")?
        .get("timeout_enabled")?
        .as_bool()
}

fn ask_user_question_timeout_secs_from_env() -> Option<u64> {
    std::env::var(ENV_ASK_USER_QUESTION_TIMEOUT_SECS)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
}

fn ask_user_question_timeout_enabled(
    config: Option<&TomlValue>,
) -> crate::agent::config::Resolved<bool> {
    use crate::agent::config::BoolFlag;
    BoolFlag::env(ENV_ASK_USER_QUESTION_TIMEOUT_ENABLED)
        .config(ask_user_question_timeout_enabled_from_toml(config))
        .default(ask_user_question::DEFAULT_ASK_USER_QUESTION_TIMEOUT_ENABLED)
        .resolve()
}

fn ask_user_question_timeout_secs_from_toml(v: Option<&TomlValue>) -> Option<u64> {
    let raw = v?
        .get("toolset")?
        .get("ask_user_question")?
        .get("timeout_secs")?
        .as_integer()?;
    let valid = u64::try_from(raw).ok().filter(|secs| *secs > 0);
    if valid.is_none() {
        tracing::warn!(
            value = raw,
            "[toolset.ask_user_question] timeout_secs must be a positive integer; ignoring value"
        );
    }
    valid
}

fn ask_user_question_timeout_secs(config: Option<&TomlValue>) -> u64 {
    ask_user_question_timeout_secs_from_env()
        .or_else(|| ask_user_question_timeout_secs_from_toml(config))
        .unwrap_or(ask_user_question::RESPONSE_TIMEOUT.as_secs())
}

pub(crate) fn resolve_ask_user_question_params_from_disk()
-> ask_user_question::AskUserQuestionParams {
    let config = crate::config::load_effective_config().ok();
    ask_user_question::AskUserQuestionParams {
        timeout_enabled: ask_user_question_timeout_enabled(config.as_ref()).value,
        timeout_secs: std::num::NonZeroU64::new(ask_user_question_timeout_secs(config.as_ref()))
            .expect("ask-user timeout resolver always returns a positive value"),
    }
}
