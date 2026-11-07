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
const ENV_ASK_USER_QUESTION_ALLOW_NON_BLOCKING: &str = "GROW_ASK_USER_QUESTION_ALLOW_NON_BLOCKING";

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

/// Typed extraction of `allow_non_blocking` from the effective TOML,
/// distinguishing a genuinely absent node/key (`Ok(None)` — may default)
/// from a present-but-invalid value or non-table intermediate node
/// (`Err` — must fail the admission, never default the gate open).
fn ask_user_question_allow_non_blocking_from_toml(
    v: Option<&TomlValue>,
) -> Result<Option<bool>, String> {
    let Some(root) = v else {
        return Ok(None);
    };
    let toolset = match root.get("toolset") {
        None => return Ok(None),
        Some(toolset) => toolset,
    };
    let ask = match toolset
        .as_table()
        .map(|table| table.get("ask_user_question"))
    {
        None => return Err("`[toolset]` must be a table".to_string()),
        Some(None) => return Ok(None),
        Some(Some(ask)) => ask,
    };
    let value = match ask.as_table().map(|table| table.get("allow_non_blocking")) {
        None => return Err("`[toolset.ask_user_question]` must be a table".to_string()),
        Some(None) => return Ok(None),
        Some(Some(value)) => value,
    };
    value.as_bool().map(Some).ok_or_else(|| {
        format!("[toolset.ask_user_question] allow_non_blocking must be a boolean, found {value}")
    })
}

/// Live-resolve `allow_non_blocking` for one ask admission: existing env tier
/// > effective global TOML (`$GROW_HOME/config.toml`, no cwd tier) > default.
///
/// A config read or syntax-parse failure returns `Err` unconditionally — the
/// file is loaded before the env short-circuit, so no environment value can
/// paper over an unreadable config. A present-but-invalid field type or
/// non-table intermediate node is also `Err` (a mistyped preference can never
/// default the gate open), but a valid env override (`true`/`false`) does win
/// over that field-level error. Timeout policy is intentionally not resolved
/// here — it stays a build-time snapshot via
/// [`resolve_ask_user_question_params_from_disk`].
pub(crate) fn resolve_ask_user_question_allow_non_blocking_from_disk() -> Result<bool, String> {
    let config = crate::config::load_effective_config().map_err(|error| error.to_string())?;
    if let Some(env) = config::env_bool(ENV_ASK_USER_QUESTION_ALLOW_NON_BLOCKING) {
        return Ok(env);
    }
    ask_user_question_allow_non_blocking_from_toml(Some(&config)).map(|value| {
        value.unwrap_or(ask_user_question::DEFAULT_ASK_USER_QUESTION_ALLOW_NON_BLOCKING)
    })
}

pub(crate) fn resolve_ask_user_question_params_from_disk()
-> ask_user_question::AskUserQuestionParams {
    let config = crate::config::load_effective_config().ok();
    ask_user_question::AskUserQuestionParams {
        timeout_enabled: ask_user_question_timeout_enabled(config.as_ref()).value,
        timeout_secs: std::num::NonZeroU64::new(ask_user_question_timeout_secs(config.as_ref()))
            .expect("ask-user timeout resolver always returns a positive value"),
        // The live resolver injected by the Factory re-resolves this per
        // admission; this value is only the build-time base and is always
        // overwritten by `UserQuestionPolicyResolver` in the same Agent.
        allow_non_blocking: ask_user_question::DEFAULT_ASK_USER_QUESTION_ALLOW_NON_BLOCKING,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_non_blocking_toml_tier_reads_only_the_ask_key() {
        let cfg: TomlValue =
            toml::from_str("[toolset.ask_user_question]\nallow_non_blocking = false\n").unwrap();
        assert_eq!(
            ask_user_question_allow_non_blocking_from_toml(Some(&cfg)),
            Ok(Some(false))
        );
        assert_eq!(
            ask_user_question_allow_non_blocking_from_toml(None),
            Ok(None)
        );
        let unrelated: TomlValue = toml::from_str("[toolset.bash]\ntimeout_secs = 1\n").unwrap();
        assert_eq!(
            ask_user_question_allow_non_blocking_from_toml(Some(&unrelated)),
            Ok(None)
        );
    }

    /// Full live resolution (env > effective TOML > default) and the
    /// fail-closed disk rule, in an isolated child with a tempdir
    /// `GROW_HOME` so the main test process never touches the real user
    /// config or a shared env var.
    #[test]
    fn allow_non_blocking_disk_resolution_tiers_and_parse_failure() {
        const MARKER: &str = "GROW_ASK_POLICY_CHILD";
        if std::env::var_os(MARKER).is_none() {
            let home = tempfile::tempdir().unwrap();
            let full = concat!(
                module_path!(),
                "::allow_non_blocking_disk_resolution_tiers_and_parse_failure"
            );
            let name = full.strip_prefix("shell::").unwrap_or(full);
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", name, "--nocapture"])
                .env("GROW_HOME", home.path())
                .env(MARKER, "1")
                .env_remove(ENV_ASK_USER_QUESTION_ALLOW_NON_BLOCKING)
                .output()
                .expect("run isolated ask policy child");
            assert!(
                output.status.success(),
                "isolated child failed\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let config_path = crate::util::config::user_config_path();
        // Default with no file on disk.
        assert_eq!(
            resolve_ask_user_question_allow_non_blocking_from_disk().unwrap(),
            ask_user_question::DEFAULT_ASK_USER_QUESTION_ALLOW_NON_BLOCKING
        );
        // Effective TOML tier.
        std::fs::write(
            &config_path,
            "[toolset.ask_user_question]\nallow_non_blocking = false\n",
        )
        .unwrap();
        assert!(!resolve_ask_user_question_allow_non_blocking_from_disk().unwrap());
        // Env tier wins over TOML.
        unsafe { std::env::set_var(ENV_ASK_USER_QUESTION_ALLOW_NON_BLOCKING, "1") };
        assert!(resolve_ask_user_question_allow_non_blocking_from_disk().unwrap());
        unsafe { std::env::remove_var(ENV_ASK_USER_QUESTION_ALLOW_NON_BLOCKING) };
        // Unparseable disk config fails closed instead of defaulting the
        // gate open — and this file-level failure is NOT overridable by a
        // valid env value: the file load happens before the env tier.
        std::fs::write(&config_path, "[toolset\nbroken").unwrap();
        let error = resolve_ask_user_question_allow_non_blocking_from_disk()
            .expect_err("disk parse failure must surface as a policy error");
        assert!(!error.is_empty());
        for env_value in ["1", "0"] {
            unsafe { std::env::set_var(ENV_ASK_USER_QUESTION_ALLOW_NON_BLOCKING, env_value) };
            resolve_ask_user_question_allow_non_blocking_from_disk().expect_err(&format!(
                "valid env {env_value} must not override a read/syntax failure"
            ));
            unsafe { std::env::remove_var(ENV_ASK_USER_QUESTION_ALLOW_NON_BLOCKING) };
        }

        // Present-but-invalid field type or structure: the resolver must
        // return Err (before any ask publication), never default the gate
        // open. Only a genuinely absent node/key may default.
        for (label, contents) in [
            (
                "string value",
                "[toolset.ask_user_question]\nallow_non_blocking = \"false\"\n",
            ),
            (
                "integer value",
                "[toolset.ask_user_question]\nallow_non_blocking = 0\n",
            ),
            (
                "array value",
                "[toolset.ask_user_question]\nallow_non_blocking = [false]\n",
            ),
            ("scalar section", "toolset = 3\n"),
            (
                "scalar ask section",
                "[toolset]\nask_user_question = \"nope\"\n",
            ),
        ] {
            std::fs::write(&config_path, contents).unwrap();
            let error = resolve_ask_user_question_allow_non_blocking_from_disk().expect_err(
                &format!("present-invalid {label} must fail, not default open"),
            );
            assert!(!error.is_empty(), "{label} error must be descriptive");
            // A valid env override still wins over the typed-invalid field.
            for (env_value, expected) in [("1", true), ("0", false)] {
                unsafe { std::env::set_var(ENV_ASK_USER_QUESTION_ALLOW_NON_BLOCKING, env_value) };
                assert_eq!(
                    resolve_ask_user_question_allow_non_blocking_from_disk().unwrap(),
                    expected,
                    "valid env {env_value} must override the typed-invalid {label}"
                );
                unsafe { std::env::remove_var(ENV_ASK_USER_QUESTION_ALLOW_NON_BLOCKING) };
            }
            // Removing the env restores the policy error.
            resolve_ask_user_question_allow_non_blocking_from_disk()
                .expect_err(&format!("removing env must restore Err for {label}"));
        }
        // Missing key still defaults (legal absence, not a typed error).
        std::fs::write(
            &config_path,
            "[toolset.ask_user_question]\ntimeout_secs = 30\n",
        )
        .unwrap();
        assert!(resolve_ask_user_question_allow_non_blocking_from_disk().unwrap());
    }
}
