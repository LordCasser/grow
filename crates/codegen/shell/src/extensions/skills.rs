use acp_transport::protocol as acp;
use serde::{Deserialize, Serialize};

use crate::util::config as cli_config;
use agent::prompt::skills::{SkillInfo, SkillsConfig, list_skills_with_plugins};

use super::ExtResult;

/// Generic params for methods that only need an optional `cwd`.
#[derive(Debug)]
struct CwdParams {
    cwd: Option<String>,
}

impl<'de> Deserialize<'de> for CwdParams {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Derived struct deserialization also accepts arrays; ACP params here
        // must be an object, including when cwd is omitted.
        let mut fields = serde_json::Map::<String, serde_json::Value>::deserialize(deserializer)?;
        let cwd = serde_json::from_value::<Option<String>>(
            fields.remove("cwd").unwrap_or(serde_json::Value::Null),
        )
        .map_err(serde::de::Error::custom)?;
        Ok(Self { cwd })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsAddRequest {
    /// Path to add (directory or SKILL.md file). Supports `~` expansion.
    pub path: String,
    /// Working directory for skill discovery context.
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsAddResponse {
    /// Number of skills discovered at the added path.
    pub added_count: usize,
    /// Total number of skills loaded across all sources.
    pub total: usize,
    /// The path that was added to config.
    pub path: String,
    /// Full updated skill list after reload.
    pub skills: Vec<SkillInfo>,
    /// Human-readable message.
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsRemoveRequest {
    /// Path to remove from config paths.
    pub path: String,
    /// Working directory for skill discovery context.
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsRemoveResponse {
    /// The path that was removed.
    pub path: String,
    /// Full updated skill list after reload.
    pub skills: Vec<SkillInfo>,
    /// Human-readable message.
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsResetResponse {
    /// Full updated skill list after reload.
    pub skills: Vec<SkillInfo>,
    /// Human-readable message.
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsToggleRequest {
    /// Catalog key to toggle: native name or plugin:name.
    pub name: String,
    /// Whether to enable (`true`) or disable (`false`) the skill.
    pub enabled: bool,
    /// Working directory for skill discovery context.
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsListRequest {
    /// Working directory for skill discovery context.
    pub cwd: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowsListRequest {
    session_id: acp::SessionId,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsListResponse {
    /// All discovered skills.
    pub skills: Vec<SkillInfo>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsConfigResponse {
    /// Configured paths from `[skills].paths`.
    pub paths: Vec<String>,
    /// Ignored paths from `[skills].ignore`.
    pub ignore: Vec<String>,
    /// Total loaded skill count.
    pub total_skills: usize,
    /// Human-readable summary.
    pub message: String,
    /// Full updated skill list.
    pub skills: Vec<SkillInfo>,
}

/// Reload skills without running filesystem scans on the async request thread.
#[tracing::instrument(skip_all, fields(cwd))]
async fn reload_skills(
    cwd: &str,
    plugin_registry: Option<&agent::plugins::PluginRegistry>,
) -> Result<Vec<SkillInfo>, acp::Error> {
    static SLOTS: std::sync::LazyLock<std::sync::Arc<tokio::sync::Semaphore>> =
        std::sync::LazyLock::new(|| std::sync::Arc::new(tokio::sync::Semaphore::new(1)));
    let cwd = cwd.to_owned();
    let registry = plugin_registry.cloned();
    let runtime = tokio::runtime::Handle::current();
    run_skill_reload(
        SLOTS.clone(),
        std::time::Duration::from_secs(5),
        move || {
            runtime.block_on(async {
                let config = cli_config::load_config().await.skills;
                list_skills_with_plugins(Some(&cwd), &config, registry.as_ref()).await
            })
        },
    )
    .await
    .map_err(|error| acp::Error::internal_error().data(format!("Skills reload failed: {error}")))
}

async fn run_skill_reload(
    slots: std::sync::Arc<tokio::sync::Semaphore>,
    timeout: std::time::Duration,
    scan: impl FnOnce() -> Vec<SkillInfo> + Send + 'static,
) -> anyhow::Result<Vec<SkillInfo>> {
    tokio::time::timeout(timeout, async move {
        let permit = slots.acquire_owned().await?;
        tokio::task::spawn_blocking(move || {
            // Request cancellation cannot interrupt filesystem calls. Keep the
            // slot until the worker actually exits, including after a timeout.
            let _permit = permit;
            scan()
        })
        .await
        .map_err(anyhow::Error::from)
    })
    .await
    .map_err(|_| anyhow::anyhow!("timed out waiting for skill discovery"))?
}

fn saved_skill_reload_error(error: acp::Error) -> acp::Error {
    acp::Error::internal_error().data(format!(
        "Skill configuration was saved, but reloading skills failed: {error}"
    ))
}

/// Count skills at or below the given path, respecting component boundaries.
fn count_skills_from(skills: &[SkillInfo], dir: &std::path::Path) -> usize {
    let dir = resolve_skill_path(&dir.to_string_lossy(), ".");
    skills
        .iter()
        .filter(|s| std::path::Path::new(&resolve_skill_path(&s.path, ".")).starts_with(&dir))
        .count()
}

fn add_skill_path(config: &mut SkillsConfig, p: String) {
    config.ignore.retain(|i| {
        let resolved = resolve_config_skill_path(i);
        let ignored = std::path::Path::new(&resolved);
        let added = std::path::Path::new(&p);
        !(added.starts_with(ignored) || ignored.starts_with(added))
    });
    if !config
        .paths
        .iter()
        .any(|i| resolve_config_skill_path(i) == p)
    {
        config.paths.push(p);
    }
}

fn remove_skill_path(config: &mut SkillsConfig, path: &str) {
    config
        .paths
        .retain(|i| resolve_config_skill_path(i) != path);
}

// Settings edits contain raw TOML strings; expand only their comparison values.
fn resolve_config_skill_path(raw: &str) -> String {
    resolve_skill_path(&crate::config::expand_env_vars_in_string(raw), ".")
}

/// Resolve a skill path to an absolute path.
///
/// Handles `~` expansion and relative path resolution against `cwd`.
/// Keeps the anchored path when the target cannot be canonicalized.
fn resolve_skill_path(raw: &str, cwd: &str) -> String {
    use std::path::PathBuf;

    // Expand ~ to $HOME
    let expanded = if let Some(rest) = raw.strip_prefix("~/") {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(|home| PathBuf::from(home).join(rest))
            .unwrap_or_else(|| PathBuf::from(raw))
    } else if raw == "~" {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(raw))
    } else {
        PathBuf::from(raw)
    };

    // If already absolute, canonicalize to resolve `..` etc.
    // If relative, join with cwd first.
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        PathBuf::from(cwd).join(&expanded)
    };

    // Anchor relative cwd even when the target does not exist. Do not collapse
    // `..` lexically: its meaning can depend on an existing symlink.
    let absolute = std::path::absolute(&absolute).unwrap_or(absolute);
    // canonicalize resolves symlinks and `..`; missing targets keep the anchor.
    dunce::canonicalize(&absolute)
        .unwrap_or(absolute)
        .to_string_lossy()
        .to_string()
}

/// Collect auto-discovered skill source directories and their counts.
fn discover_auto_sources(cwd: &str, skills: &[SkillInfo]) -> Vec<(String, usize)> {
    let cwd_path = std::path::PathBuf::from(cwd);
    let grow_home = tools::util::grow_home::grow_home();
    let git_root = git2::Repository::discover(&cwd_path)
        .ok()
        .and_then(|repo| repo.workdir().map(|p| p.to_path_buf()));

    let mut sources: Vec<(String, usize)> = Vec::new();
    let subdirs = ["skills", "commands"];

    let mut try_add_source = |dir: std::path::PathBuf, seen: Option<&[std::path::PathBuf]>| {
        if dir.is_dir() && !seen.is_some_and(|s| s.contains(&dir)) {
            let count = count_skills_from(skills, &dir);
            if count > 0 {
                sources.push((dir.to_string_lossy().to_string(), count));
            }
        }
    };

    let mut local_dirs: Vec<std::path::PathBuf> = Vec::new();
    for subdir in &subdirs {
        let dir = cwd_path.join(".grow").join(subdir);
        try_add_source(dir.clone(), None);
        local_dirs.push(dir);
    }

    if let Some(ref root) = git_root {
        for subdir in &subdirs {
            try_add_source(root.join(".grow").join(subdir), Some(&local_dirs));
        }
    }

    for subdir in &subdirs {
        try_add_source(grow_home.join(subdir), None);
    }

    // Explicit custom skill roots supplement canonical discovery.
    for dir in extra_skill_dirs_from_config() {
        let path = crate::util::expand_home(&dir);
        if path.is_dir()
            && !sources
                .iter()
                .any(|(s, _)| s.as_str() == path.to_string_lossy().as_ref())
        {
            sources.push((
                path.to_string_lossy().to_string(),
                count_skills_from(skills, &path),
            ));
        }
    }

    sources
}

/// Read `[paths] extra_skill_dirs` from the effective config. Returns empty
/// on any read/parse failure so misconfiguration never breaks listing.
fn extra_skill_dirs_from_config() -> Vec<String> {
    let Ok(root) = crate::config::load_effective_config() else {
        return Vec::new();
    };
    root.get("paths")
        .and_then(|v| v.get("extra_skill_dirs"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

#[tracing::instrument(skip_all, fields(method = %args.method))]
pub async fn handle(
    agent: &crate::agent::mvp_agent::MvpAgent,
    args: &acp::ExtRequest,
    plugin_registry: Option<&agent::plugins::PluginRegistry>,
) -> ExtResult {
    match args.method.as_ref() {
        "grow/skills/add" => {
            let req: SkillsAddRequest = serde_json::from_str(args.params.get())?;
            let cwd = req.cwd.as_deref().unwrap_or(".");

            // Resolve to absolute path so config entries work from any cwd.
            let resolved = resolve_skill_path(&req.path, cwd);

            let p = resolved.clone();
            if let Err(e) = cli_config::update_config(|cfg| {
                add_skill_path(&mut cfg.skills, p);
            })
            .await
            {
                ::diagnostics::session_ctx::log_event(::diagnostics::events::SkillAdded {
                    added_count: 0,
                    total_skills: 0,
                    success: false,
                });
                return super::to_ext_response(Err::<SkillsAddResponse, _>(anyhow::anyhow!(
                    "Failed to save config: {e}"
                )));
            }

            let skills = reload_skills(cwd, plugin_registry)
                .await
                .map_err(saved_skill_reload_error)?;
            let added_count = count_skills_from(&skills, std::path::Path::new(&resolved));
            let total = skills.len();
            let message = format!(
                "Added path {}. {} new skill{} found ({} total).",
                resolved,
                added_count,
                if added_count == 1 { "" } else { "s" },
                total,
            );

            ::diagnostics::session_ctx::log_event(::diagnostics::events::SkillAdded {
                added_count: added_count as u32,
                total_skills: total as u32,
                success: true,
            });
            super::to_ext_response(Ok(SkillsAddResponse {
                added_count,
                total,
                path: resolved,
                skills,
                message,
            }))
        }

        "grow/skills/remove" => {
            let req: SkillsRemoveRequest = serde_json::from_str(args.params.get())?;
            let cwd = req.cwd.as_deref().unwrap_or(".");

            // Resolve so relative/tilde paths match what was saved by add.
            let resolved = resolve_skill_path(&req.path, cwd);

            let p = resolved.clone();
            if let Err(e) = cli_config::update_config(|cfg| {
                remove_skill_path(&mut cfg.skills, &p);
            })
            .await
            {
                ::diagnostics::session_ctx::log_event(::diagnostics::events::SkillRemoved {
                    success: false,
                });
                return super::to_ext_response(Err::<SkillsRemoveResponse, _>(anyhow::anyhow!(
                    "Failed to save config: {e}"
                )));
            }

            let skills = reload_skills(cwd, plugin_registry)
                .await
                .map_err(saved_skill_reload_error)?;
            let total = skills.len();
            let message = format!(
                "Removed path {}. {} skill{} remaining.",
                resolved,
                total,
                if total == 1 { "" } else { "s" },
            );

            ::diagnostics::session_ctx::log_event(::diagnostics::events::SkillRemoved {
                success: true,
            });
            super::to_ext_response(Ok(SkillsRemoveResponse {
                path: resolved,
                skills,
                message,
            }))
        }

        "grow/skills/reset" => {
            let params: CwdParams = super::parse_params(args)?;
            let cwd = params.cwd.as_deref().unwrap_or(".");

            if let Err(e) = cli_config::update_config(|cfg| {
                cfg.skills = SkillsConfig::default();
            })
            .await
            {
                return super::to_ext_response(Err::<SkillsResetResponse, _>(anyhow::anyhow!(
                    "Failed to save config: {e}"
                )));
            }

            let skills = reload_skills(cwd, plugin_registry)
                .await
                .map_err(saved_skill_reload_error)?;
            let message = "Custom skills config reset".to_string();

            super::to_ext_response(Ok(SkillsResetResponse { skills, message }))
        }

        "grow/skills/list" => {
            let req: SkillsListRequest = serde_json::from_str(args.params.get())?;
            let skills = reload_skills(&req.cwd, plugin_registry).await?;
            super::to_ext_response(Ok(SkillsListResponse { skills }))
        }

        "grow/workflows/list" => {
            let req: WorkflowsListRequest = serde_json::from_str(args.params.get())?;
            let Some(handle) = agent.session_handle_waiting_for_load(&req.session_id).await else {
                return super::to_ext_response(Err::<serde_json::Value, _>(anyhow::anyhow!(
                    "unknown session id: {}",
                    req.session_id.0
                )));
            };
            let (launches_enabled, _management_available) = handle.workflow_catalog_state().await;
            let workflows = if launches_enabled {
                crate::session::workflow::registry::list_workflows(Some(
                    handle.tool_context.cwd.as_path(),
                ))
            } else {
                Vec::new()
            };
            super::to_ext_response(Ok(serde_json::json!({ "workflows": workflows })))
        }

        "grow/skills/config" => {
            let params: CwdParams = super::parse_params(args)?;
            let cwd = params.cwd.as_deref().unwrap_or(".");

            let config = cli_config::load_config().await.skills;
            let paths = config.paths.clone();
            let ignore = config.ignore.clone();

            let skills = reload_skills(cwd, plugin_registry).await?;
            let total_skills = skills.len();

            let auto_sources = discover_auto_sources(cwd, &skills);

            let mut msg = String::new();

            msg.push_str("Skill discovery sources:\n");
            for (source, count) in &auto_sources {
                msg.push_str(&format!(
                    "  • {}  ({} skill{})\n",
                    source,
                    count,
                    if *count == 1 { "" } else { "s" }
                ));
            }
            if auto_sources.is_empty() {
                msg.push_str("  (no auto-discovered directories found)\n");
            }

            if !paths.is_empty() {
                msg.push_str("\nCustom paths:\n");
                for p in &paths {
                    let count = count_skills_from(&skills, std::path::Path::new(p));
                    msg.push_str(&format!(
                        "  • {}  ({} skill{})\n",
                        p,
                        count,
                        if count == 1 { "" } else { "s" }
                    ));
                }
            }

            if !ignore.is_empty() {
                msg.push_str("\nIgnored:\n");
                for p in &ignore {
                    msg.push_str(&format!("  • {}\n", p));
                }
            }

            msg.push_str(&format!("\nTotal skills loaded: {}", total_skills));

            super::to_ext_response(Ok(SkillsConfigResponse {
                paths,
                ignore,
                total_skills,
                message: msg,
                skills,
            }))
        }

        "grow/skills/toggle" => {
            let req: SkillsToggleRequest = serde_json::from_str(args.params.get())?;
            let cwd = req.cwd.as_deref().unwrap_or(".");

            // Validate the skill name exists before modifying config.
            let current_skills = reload_skills(cwd, plugin_registry).await?;
            if !current_skills.iter().any(|s| s.dedup_key() == req.name) {
                return super::to_ext_response(Err::<SkillsListResponse, _>(anyhow::anyhow!(
                    "Skill '{}' not found",
                    req.name
                )));
            }

            let name = req.name.clone();
            let enabled = req.enabled;
            if let Err(e) = cli_config::update_config(|cfg| {
                if enabled {
                    cfg.skills.disabled.retain(|d| d != &name);
                } else if !cfg.skills.disabled.contains(&name) {
                    cfg.skills.disabled.push(name.clone());
                }
            })
            .await
            {
                return super::to_ext_response(Err::<SkillsListResponse, _>(anyhow::anyhow!(
                    "Failed to save config: {e}"
                )));
            }

            // Re-apply disabled marking against the already-loaded skills
            // to reflect the config change without a second full discovery.
            let config = cli_config::load_config().await.skills;
            let disabled_set: std::collections::HashSet<&str> =
                config.disabled.iter().map(|s| s.as_str()).collect();
            let skills: Vec<SkillInfo> = current_skills
                .into_iter()
                .map(|mut s| {
                    s.enabled = !disabled_set.contains(s.dedup_key().as_str());
                    s
                })
                .collect();
            super::to_ext_response(Ok(SkillsListResponse { skills }))
        }

        _ => Err(acp::Error::method_not_found()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_request_with_cwd() {
        let json = r#"{"path": "/home/user/skills", "cwd": "/project"}"#;
        let req: SkillsAddRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.path, "/home/user/skills");
        assert_eq!(req.cwd, Some("/project".to_string()));
    }

    #[test]
    fn test_add_request_without_cwd() {
        let json = r#"{"path": "~/my-skills"}"#;
        let req: SkillsAddRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.path, "~/my-skills");
        assert_eq!(req.cwd, None);
    }

    #[test]
    fn test_remove_request() {
        let json = r#"{"path": "/home/user/skills", "cwd": "/project"}"#;
        let req: SkillsRemoveRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.path, "/home/user/skills");
        assert_eq!(req.cwd, Some("/project".to_string()));
    }

    #[test]
    fn test_list_request() {
        let json = r#"{"cwd": "/project"}"#;
        let req: SkillsListRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.cwd, "/project");
    }

    #[test]
    fn test_add_response_camel_case() {
        let resp = SkillsAddResponse {
            added_count: 3,
            total: 10,
            path: "/test".to_string(),
            skills: vec![],
            message: "ok".to_string(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["addedCount"], 3);
        assert_eq!(json["total"], 10);
        assert_eq!(json["path"], "/test");
    }

    #[test]
    fn test_resolve_absolute_path_unchanged() {
        let resolved = resolve_skill_path("/absolute/path/to/skills", "/some/cwd");
        // Canonicalize will fail (path doesn't exist), so we get the joined absolute path
        assert_eq!(resolved, "/absolute/path/to/skills");
    }

    #[test]
    fn test_resolve_relative_path_against_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("sub");
        std::fs::create_dir(&sub).unwrap();

        let resolved = resolve_skill_path("sub", &tmp.path().to_string_lossy());
        assert_eq!(
            resolved,
            dunce::canonicalize(&sub).unwrap().to_string_lossy()
        );
    }

    #[test]
    fn test_resolve_dotdot_path() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().join("a").join("b");
        std::fs::create_dir_all(&cwd).unwrap();

        let resolved = resolve_skill_path("../..", &cwd.to_string_lossy());
        assert_eq!(
            resolved,
            dunce::canonicalize(tmp.path()).unwrap().to_string_lossy()
        );
    }

    /// Hermetic tilde expansion: pin HOME to a temp dir so remote sandboxes
    /// (missing HOME, symlink-resolved homes, pre-existing ~/my-skills) cannot
    /// make `starts_with($HOME)` fail spuriously. Serial because env mutation
    /// is process-global.
    #[test]
    #[serial_test::serial]
    fn test_resolve_tilde_path() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().to_path_buf();
        let prev_home = std::env::var_os("HOME");
        let prev_userprofile = std::env::var_os("USERPROFILE");
        // SAFETY: serial test; restored in the same scope below.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::remove_var("USERPROFILE");
        }
        let resolved = resolve_skill_path("~/my-skills", "/ignored");
        match prev_home {
            Some(v) => unsafe { std::env::set_var("HOME", v) },
            None => unsafe { std::env::remove_var("HOME") },
        }
        match prev_userprofile {
            Some(v) => unsafe { std::env::set_var("USERPROFILE", v) },
            None => unsafe { std::env::remove_var("USERPROFILE") },
        }
        let expected = home.join("my-skills");
        assert_eq!(
            std::path::PathBuf::from(&resolved),
            expected,
            "resolved={resolved}"
        );
    }

    #[test]
    fn test_config_response_camel_case() {
        let resp = SkillsConfigResponse {
            paths: vec!["/a".into()],
            ignore: vec![],
            total_skills: 5,
            message: "ok".to_string(),
            skills: vec![],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["totalSkills"], 5);
        assert!(json["paths"].is_array());
    }
    #[test]
    fn add_preserves_neighbor_ignore_paths() {
        let mut config = SkillsConfig {
            ignore: [
                "/skills/foobar",
                "/skills/foo",
                "/skills/foo/child",
                "/skills",
                "/other",
            ]
            .map(str::to_owned)
            .to_vec(),
            ..Default::default()
        };
        add_skill_path(&mut config, "/skills/foo".into());
        add_skill_path(&mut config, "/skills/foo".into());
        assert_eq!(config.ignore, ["/skills/foobar", "/other"]);
        assert_eq!(config.paths, ["/skills/foo"]);
    }

    #[test]
    fn count_excludes_neighbor_prefixes() {
        let skills = [
            "/skills/foo/SKILL.md",
            "/skills/foobar/SKILL.md",
            "/skills/foo/nested/SKILL.md",
        ]
        .map(|path| SkillInfo {
            path: path.into(),
            ..Default::default()
        });
        assert_eq!(
            count_skills_from(&skills, std::path::Path::new("/skills/foo")),
            2
        );
        assert_eq!(
            count_skills_from(&skills, std::path::Path::new("/skills/foo/SKILL.md")),
            1
        );
    }
    #[cfg(unix)]
    #[test]
    fn existing_symlink_alias_management() {
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real");
        let alias = temp.path().join("alias");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let alias_text = alias.to_string_lossy().into_owned();
        let resolved = resolve_skill_path(real.to_str().unwrap(), ".");
        let mut config = SkillsConfig {
            paths: vec![alias_text.clone()],
            ignore: vec![alias_text.clone()],
            ..Default::default()
        };
        add_skill_path(&mut config, resolved.clone());
        assert!(config.ignore.is_empty());
        assert_eq!(config.paths, [alias_text]);
        remove_skill_path(&mut config, &resolved);
        assert!(config.paths.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn source_count_resolves_existing_alias() {
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real");
        let alias = temp.path().join("alias");
        std::fs::create_dir(&real).unwrap();
        std::fs::write(real.join("SKILL.md"), "body").unwrap();
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let skills = [SkillInfo {
            path: resolve_skill_path(real.join("SKILL.md").to_str().unwrap(), "."),
            ..Default::default()
        }];
        assert_eq!(count_skills_from(&skills, &alias), 1);
    }
    #[test]
    fn manages_environment_paths_without_rewriting_config() {
        let home = std::env::var("HOME").expect("HOME available for read-only fixture");
        let resolved = resolve_skill_path(&home, ".");
        let mut config = SkillsConfig {
            paths: vec!["${HOME}".into()],
            ignore: vec!["${HOME}".into()],
            ..Default::default()
        };
        add_skill_path(&mut config, resolved.clone());
        assert!(config.ignore.is_empty());
        assert_eq!(config.paths, ["${HOME}"]);
        remove_skill_path(&mut config, &resolved);
        assert!(config.paths.is_empty());
    }

    #[test]
    fn request_path_keeps_environment_text_literal() {
        let temp = tempfile::tempdir().unwrap();
        let resolved = resolve_skill_path("${HOME}/skill", temp.path().to_str().unwrap());
        assert!(resolved.contains("${HOME}"));
    }
    #[test]
    fn missing_skill_path_is_anchored_with_relative_cwd() {
        let current = std::env::current_dir().unwrap();
        let name = format!("missing-skill-{}", uuid::Uuid::new_v4());
        assert!(!current.join(&name).exists());
        for cwd in [".", "relative-workspace"] {
            let resolved = resolve_skill_path(&name, cwd);
            assert!(std::path::Path::new(&resolved).is_absolute(), "{resolved}");
            assert_eq!(
                std::path::PathBuf::from(&resolved),
                std::path::absolute(current.join(cwd).join(&name)).unwrap()
            );
            let mut config = SkillsConfig::default();
            add_skill_path(&mut config, resolved.clone());
            assert_eq!(config.paths, [resolved]);
        }
    }
    #[test]
    fn optional_cwd_rejects_invalid_params_and_accepts_defaults() {
        for raw in [
            "null",
            "[]",
            r#"["/project"]"#,
            r#"{"cwd":1}"#,
            r#"{"cwd":false}"#,
        ] {
            let error = super::super::parse_params_str::<CwdParams>(raw).unwrap_err();
            assert_eq!(error.code, acp::Error::invalid_params().code, "{raw}");
        }
        for raw in ["{}", r#"{"cwd":null}"#] {
            let params = super::super::parse_params_str::<CwdParams>(raw).unwrap();
            assert!(params.cwd.is_none());
        }
        let params = super::super::parse_params_str::<CwdParams>(r#"{"cwd":"/project"}"#).unwrap();
        assert_eq!(params.cwd.as_deref(), Some("/project"));
    }
    #[tokio::test]
    async fn reload_timeout_retains_slot_until_worker_exits() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        use std::time::Duration;
        let slots = Arc::new(tokio::sync::Semaphore::new(1));
        let (started, start) = tokio::sync::oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        let first = tokio::spawn(run_skill_reload(
            slots.clone(),
            Duration::from_millis(50),
            move || {
                let _ = started.send(());
                let _ = released.recv();
                vec![]
            },
        ));
        start.await.unwrap();
        assert!(first.await.unwrap().is_err());
        assert_eq!(slots.available_permits(), 0);
        let ran = Arc::new(AtomicBool::new(false));
        let observed = ran.clone();
        assert!(
            run_skill_reload(slots.clone(), Duration::from_millis(10), move || {
                observed.store(true, Ordering::SeqCst);
                vec![]
            })
            .await
            .is_err()
        );
        assert!(!ran.load(Ordering::SeqCst));
        release.send(()).unwrap();
        let permit = tokio::time::timeout(Duration::from_secs(2), slots.acquire())
            .await
            .unwrap()
            .unwrap();
        drop(permit);
        assert!(!ran.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn reload_empty_success_and_worker_failure_are_distinct() {
        use std::{sync::Arc, time::Duration};
        let slots = Arc::new(tokio::sync::Semaphore::new(1));
        assert!(
            run_skill_reload(slots.clone(), Duration::from_secs(2), Vec::new)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            run_skill_reload(slots.clone(), Duration::from_secs(2), || panic!(
                "scan failed"
            ))
            .await
            .is_err()
        );
        assert_eq!(slots.available_permits(), 1);
    }

    #[tokio::test]
    async fn canceled_reload_waiter_does_not_start_scan() {
        use std::{
            sync::{
                Arc,
                atomic::{AtomicBool, Ordering},
            },
            time::Duration,
        };
        let slots = Arc::new(tokio::sync::Semaphore::new(1));
        let permit = slots.clone().acquire_owned().await.unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        let observed = ran.clone();
        let waiter = tokio::spawn(run_skill_reload(
            slots.clone(),
            Duration::from_secs(2),
            move || {
                observed.store(true, Ordering::SeqCst);
                vec![]
            },
        ));
        tokio::task::yield_now().await;
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        drop(permit);
        let permit = slots.acquire().await.unwrap();
        assert!(!ran.load(Ordering::SeqCst));
        drop(permit);
    }
}
