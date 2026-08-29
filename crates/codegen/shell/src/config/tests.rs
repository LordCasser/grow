#![cfg_attr(rustfmt, rustfmt::skip)]
use super::*;
fn with_env_var<T>(name: &str, value: &str, f: impl FnOnce() -> T) -> T {
    let previous = std::env::var(name).ok();
    unsafe {
        std::env::set_var(name, value);
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    match previous {
        Some(prev) => {
            unsafe {
                std::env::set_var(name, prev);
            }
        }
        None => {
            unsafe {
                std::env::remove_var(name);
            }
        }
    }
    match result {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}
#[test]
fn expands_env_vars_in_toml_strings() {
    with_env_var(
        "GROW_TEST_CONFIG_EXPAND",
        "expanded",
        || {
            let toml_str = r#"
[mcp_servers.test]
command = "$GROW_TEST_CONFIG_EXPAND/bin/server"
args = ["--path", "${GROW_TEST_CONFIG_EXPAND}/data"]
"#;
            let mut value = toml::from_str::<toml::Value>(toml_str).unwrap();
            expand_env_vars_in_toml(&mut value);
            let toml::Value::Table(table) = value else {
                panic!("Expected table root");
            };
            let Some(toml::Value::Table(mcp_servers)) = table.get("mcp_servers") else {
                panic!("Expected mcp_servers table");
            };
            let Some(toml::Value::Table(test)) = mcp_servers.get("test") else {
                panic!("Expected test table");
            };
            let command = test.get("command").and_then(|v| v.as_str()).unwrap();
            assert_eq!(command, "expanded/bin/server");
            let args = test.get("args").and_then(|v| v.as_array()).unwrap();
            assert_eq!(args[1].as_str().unwrap(), "expanded/data");
        },
    );
}
#[test]
fn leaves_missing_env_vars_unchanged() {
    let toml_str = r#"
[mcp_servers.test]
command = "$GROW_TEST_CONFIG_MISSING/bin/server"
"#;
    let mut value = toml::from_str::<toml::Value>(toml_str).unwrap();
    expand_env_vars_in_toml(&mut value);
    let toml::Value::Table(table) = value else {
        panic!("Expected table root");
    };
    let Some(toml::Value::Table(mcp_servers)) = table.get("mcp_servers") else {
        panic!("Expected mcp_servers table");
    };
    let Some(toml::Value::Table(test)) = mcp_servers.get("test") else {
        panic!("Expected test table");
    };
    let command = test.get("command").and_then(|v| v.as_str()).unwrap();
    assert_eq!(command, "$GROW_TEST_CONFIG_MISSING/bin/server");
}
#[test]
fn preserves_literal_dollar_signs() {
    let toml_str = r#"
[mcp_servers.test]
command = "$$HOME"
"#;
    let mut value = toml::from_str::<toml::Value>(toml_str).unwrap();
    expand_env_vars_in_toml(&mut value);
    let toml::Value::Table(table) = value else {
        panic!("Expected table root");
    };
    let Some(toml::Value::Table(mcp_servers)) = table.get("mcp_servers") else {
        panic!("Expected mcp_servers table");
    };
    let Some(toml::Value::Table(test)) = mcp_servers.get("test") else {
        panic!("Expected test table");
    };
    let command = test.get("command").and_then(|v| v.as_str()).unwrap();
    assert_eq!(command, "$HOME");
}
/// Mutex to serialize tests that touch the GROW_MEMORY env var.
/// Env vars are process-global, so parallel tests race on them.
static MEMORY_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// Run `f` with `name` set to `value` (Some) or removed (None).
/// Saves and restores the previous value, even on panic.
fn with_env_var_opt<T>(name: &str, value: Option<&str>, f: impl FnOnce() -> T) -> T {
    let previous = std::env::var(name).ok();
    match value {
        Some(v) => unsafe { std::env::set_var(name, v) }
        None => unsafe { std::env::remove_var(name) }
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    match previous {
        Some(prev) => unsafe { std::env::set_var(name, prev) }
        None => unsafe { std::env::remove_var(name) }
    }
    result.unwrap_or_else(|p| std::panic::resume_unwind(p))
}
/// Run `f` with GROW_MEMORY explicitly unset.
fn without_memory<T>(f: impl FnOnce() -> T) -> T {
    let _guard = MEMORY_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    with_env_var_opt("GROW_MEMORY", None, f)
}
/// Run `f` with GROW_MEMORY set to a specific value.
fn with_memory<T>(value: &str, f: impl FnOnce() -> T) -> T {
    let _guard = MEMORY_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    with_env_var_opt("GROW_MEMORY", Some(value), f)
}
#[test]
fn memory_config_default_disabled() {
    without_memory(|| {
        let config = toml::Value::Table(toml::map::Map::new());
        let mem = MemoryConfig::resolve(false, false, &config, None);
        assert!(!mem.enabled);
    });
}
#[test]
fn memory_config_cli_flag_enables() {
    without_memory(|| {
        let config = toml::Value::Table(toml::map::Map::new());
        let mem = MemoryConfig::resolve(true, false, &config, None);
        assert!(mem.enabled);
    });
}
#[test]
fn memory_config_from_toml() {
    without_memory(|| {
        let config: toml::Value = toml::from_str("[memory]\nenabled = true").unwrap();
        let mem = MemoryConfig::resolve(false, false, &config, None);
        assert!(mem.enabled);
    });
}
#[test]
fn memory_config_toml_disabled() {
    without_memory(|| {
        let config: toml::Value = toml::from_str("[memory]\nenabled = false").unwrap();
        let mem = MemoryConfig::resolve(false, false, &config, None);
        assert!(!mem.enabled);
    });
}
#[test]
fn memory_config_env_var_enables() {
    with_memory(
        "1",
        || {
            let config = toml::Value::Table(toml::map::Map::new());
            let mem = MemoryConfig::resolve(false, false, &config, None);
            assert!(mem.enabled);
        },
    );
}
#[test]
fn memory_config_env_var_true_enables() {
    with_memory(
        "true",
        || {
            let config = toml::Value::Table(toml::map::Map::new());
            let mem = MemoryConfig::resolve(false, false, &config, None);
            assert!(mem.enabled);
        },
    );
}
#[test]
fn memory_config_env_var_zero_does_not_enable() {
    with_memory(
        "0",
        || {
            let config = toml::Value::Table(toml::map::Map::new());
            let mem = MemoryConfig::resolve(false, false, &config, None);
            assert!(!mem.enabled, "GROW_MEMORY=0 should not enable memory");
        },
    );
}
#[test]
fn memory_config_env_var_false_does_not_enable() {
    with_memory(
        "false",
        || {
            let config = toml::Value::Table(toml::map::Map::new());
            let mem = MemoryConfig::resolve(false, false, &config, None);
            assert!(!mem.enabled, "GROW_MEMORY=false should not enable memory");
        },
    );
}
#[test]
fn memory_config_cli_overrides_toml_disabled() {
    without_memory(|| {
        let config: toml::Value = toml::from_str("[memory]\nenabled = false").unwrap();
        let mem = MemoryConfig::resolve(true, false, &config, None);
        assert!(mem.enabled, "CLI flag should override config file");
    });
}
#[test]
fn memory_config_env_zero_force_disables_toml_enabled() {
    with_memory(
        "0",
        || {
            let config: toml::Value = toml::from_str("[memory]\nenabled = true")
                .unwrap();
            let mem = MemoryConfig::resolve(false, false, &config, None);
            assert!(
                !mem.enabled,
                "GROW_MEMORY=0 should force-disable even when TOML enables memory"
            );
        },
    );
}
#[test]
fn memory_config_env_false_force_disables_toml_enabled() {
    with_memory(
        "false",
        || {
            let config: toml::Value = toml::from_str("[memory]\nenabled = true")
                .unwrap();
            let mem = MemoryConfig::resolve(false, false, &config, None);
            assert!(
                !mem.enabled,
                "GROW_MEMORY=false should force-disable even when TOML enables memory"
            );
        },
    );
}
#[test]
fn memory_config_cli_flag_overrides_env_disable() {
    with_memory(
        "0",
        || {
            let config = toml::Value::Table(toml::map::Map::new());
            let mem = MemoryConfig::resolve(true, false, &config, None);
            assert!(
                mem.enabled,
                "CLI --experimental-memory should override GROW_MEMORY=0"
            );
        },
    );
}
#[test]
fn memory_config_no_memory_overrides_all() {
    with_memory(
        "1",
        || {
            let config: toml::Value = toml::from_str("[memory]\nenabled = true")
                .unwrap();
            let mem = MemoryConfig::resolve(true, true, &config, None);
            assert!(
                !mem.enabled,
                "--no-memory should override --experimental-memory, GROW_MEMORY=1, and TOML enabled=true"
            );
        },
    );
}
#[test]
fn memory_config_no_memory_alone_disables() {
    without_memory(|| {
        let config = toml::Value::Table(toml::map::Map::new());
        let mem = MemoryConfig::resolve(false, true, &config, None);
        assert!(!mem.enabled, "--no-memory alone should disable");
    });
}
#[test]
fn memory_config_no_memory_overrides_env_enable() {
    with_memory(
        "1",
        || {
            let config = toml::Value::Table(toml::map::Map::new());
            let mem = MemoryConfig::resolve(false, true, &config, None);
            assert!(!mem.enabled, "--no-memory should override GROW_MEMORY=1");
        },
    );
}
#[test]
fn memory_config_no_memory_overrides_toml_enabled() {
    without_memory(|| {
        let config: toml::Value = toml::from_str("[memory]\nenabled = true").unwrap();
        let mem = MemoryConfig::resolve(false, true, &config, None);
        assert!(
                !mem.enabled,
                "--no-memory should override TOML enabled=true"
            );
    });
}
#[test]
fn memory_config_no_memory_overrides_remote_enabled() {
    without_memory(|| {
        let config = toml::Value::Table(toml::map::Map::new());
        let remote = crate::util::config::RemoteSettings {
            memory_enabled: Some(true),
            ..Default::default()
        };
        let mem = MemoryConfig::resolve(false, true, &config, Some(&remote));
        assert!(
                !mem.enabled,
                "--no-memory should override remote memory_enabled=true"
            );
    });
}
#[test]
fn memory_config_defaults_are_correct() {
    without_memory(|| {
        let config = toml::Value::Table(toml::map::Map::new());
        let mem = MemoryConfig::resolve(false, false, &config, None);
        assert_eq!(mem.index.max_chunk_chars, 1600);
        assert_eq!(mem.index.chunk_overlap_chars, 320);
        assert_eq!(mem.embedding.provider, "api");
        assert_eq!(mem.embedding.model, None);
        assert_eq!(mem.embedding.dimensions, 1024);
        assert_eq!(mem.search.max_results, 6);
        assert!((mem.search.min_score - 0.35).abs() < f32::EPSILON);
        assert!((mem.search.vector_weight - 0.7).abs() < f32::EPSILON);
        assert!((mem.search.text_weight - 0.3).abs() < f32::EPSILON);
        assert!(mem.search.temporal_decay.enabled);
        assert!((mem.search.temporal_decay.half_life_days - 7.0).abs() < f64::EPSILON);
        assert!(!mem.search.mmr.enabled);
        assert!((mem.search.mmr.lambda - 0.7).abs() < f64::EPSILON);
        assert!((mem.search.source_weights["workspace"] - 1.0).abs() < f32::EPSILON);
        assert!((mem.search.source_weights["session"] - 1.0).abs() < f32::EPSILON);
        assert!((mem.search.source_weights["global"] - 1.0).abs() < f32::EPSILON);
        assert!(mem.initial_injection.enabled);
        assert_eq!(mem.initial_injection.min_score, None);
        assert!(mem.session.save_on_end);
        assert!(mem.flush.enabled);
        assert_eq!(mem.flush.soft_threshold_tokens, 4000);
        assert!(mem.flush.flush_model.is_none());
        assert_eq!(mem.flush.max_flush_write_chars, 8000);
        assert!(mem.flush.idle_timeout_secs.is_none());
        assert!(mem.watcher.enabled);
        assert_eq!(mem.watcher.stale_claim_secs, 60);
        assert!(mem.dream.enabled);
        assert_eq!(mem.dream.min_hours, 4);
        assert_eq!(mem.dream.min_sessions, 3);
        assert_eq!(mem.dream.stale_lock_secs, 3600);
        assert_eq!(mem.dream.check_interval_secs, None);
    });
}
#[test]
fn memory_watcher_rejects_removed_debounce_ms() {
    assert!(
        toml::from_str::<MemoryWatcherConfig>("enabled = true\ndebounce_ms = 2000\n").is_err()
    );
}
#[test]
fn memory_config_full_toml_parsing() {
    without_memory(|| {
        let toml_str = r#"
[memory]
enabled = true

[memory.index]
max_chunk_chars = 2000
chunk_overlap_chars = 400

[memory.embedding]
provider = "local"
model = "all-MiniLM-L6-v2"
dimensions = 384

[memory.search]
max_results = 10
min_score = 0.5
vector_weight = 0.6
text_weight = 0.4

[memory.initial_injection]
enabled = false
min_score = 0.8

[memory.search.temporal_decay]
enabled = true
half_life_days = 14.0

[memory.search.source_weights]
workspace = 1.0
session = 0.8
global = 0.5

[memory.session]
save_on_end = false

[compaction.memory_flush]
enabled = false
soft_threshold_tokens = 8000
flush_model = "grow-4"
max_flush_write_chars = 16000
idle_timeout_secs = 300
semantic_dedup_threshold = 0.85

"#;
        let config: toml::Value = toml::from_str(toml_str).unwrap();
        let mem = MemoryConfig::resolve(false, false, &config, None);
        assert!(mem.enabled);
        assert_eq!(mem.index.max_chunk_chars, 2000);
        assert_eq!(mem.index.chunk_overlap_chars, 400);
        assert_eq!(mem.embedding.provider, "local");
        assert_eq!(
                mem.embedding.model.as_deref(),
                Some("all-MiniLM-L6-v2")
            );
        assert_eq!(mem.embedding.dimensions, 384);
        assert_eq!(mem.search.max_results, 10);
        assert!((mem.search.min_score - 0.5).abs() < f32::EPSILON);
        assert!(!mem.initial_injection.enabled);
        assert_eq!(mem.initial_injection.min_score, Some(0.8));
        assert!(mem.search.temporal_decay.enabled);
        assert!((mem.search.temporal_decay.half_life_days - 14.0).abs() < f64::EPSILON);
        assert!((mem.search.source_weights["global"] - 0.5).abs() < f32::EPSILON);
        assert!(!mem.session.save_on_end);
        assert!(!mem.flush.enabled);
        assert_eq!(mem.flush.soft_threshold_tokens, 8000);
        assert_eq!(mem.flush.flush_model.as_deref(), Some("grow-4"));
        assert_eq!(mem.flush.max_flush_write_chars, 16000);
        assert_eq!(mem.flush.idle_timeout_secs, Some(300));
        assert_eq!(mem.flush.semantic_dedup_threshold, Some(0.85));
    });
}
#[test]
fn memory_config_partial_toml_uses_defaults_for_missing() {
    without_memory(|| {
        let toml_str = r#"
[memory]
enabled = true

[memory.index]
max_chunk_chars = 3200
"#;
        let config: toml::Value = toml::from_str(toml_str).unwrap();
        let mem = MemoryConfig::resolve(false, false, &config, None);
        assert!(mem.enabled);
        assert_eq!(mem.index.max_chunk_chars, 3200);
        assert_eq!(mem.index.chunk_overlap_chars, 320);
        assert_eq!(mem.embedding.dimensions, 1024);
        assert_eq!(mem.search.max_results, 6);
        assert!(mem.flush.enabled);
    });
}
#[test]
fn memory_config_remote_settings_enable() {
    without_memory(|| {
        let config = toml::Value::Table(toml::map::Map::new());
        let remote = crate::util::config::RemoteSettings {
            memory_enabled: Some(true),
            ..Default::default()
        };
        let mem = MemoryConfig::resolve(false, false, &config, Some(&remote));
        assert!(
                mem.enabled,
                "remote memory_enabled=true should enable memory"
            );
    });
}
#[test]
fn memory_config_remote_settings_initial_injection() {
    without_memory(|| {
        let config = toml::Value::Table(toml::map::Map::new());
        let remote = crate::util::config::RemoteSettings {
            memory_initial_injection_enabled: Some(false),
            memory_initial_injection_min_score: Some(0.77),
            ..Default::default()
        };
        let mem = MemoryConfig::resolve(false, false, &config, Some(&remote));
        assert!(!mem.initial_injection.enabled);
        assert_eq!(mem.initial_injection.min_score, Some(0.77));
    });
}
#[test]
fn memory_config_local_initial_injection_overrides_remote() {
    without_memory(|| {
        let toml_str = r#"
[memory.initial_injection]
enabled = true
min_score = 0.25
"#;
        let config: toml::Value = toml::from_str(toml_str).unwrap();
        let remote = crate::util::config::RemoteSettings {
            memory_initial_injection_enabled: Some(false),
            memory_initial_injection_min_score: Some(0.77),
            ..Default::default()
        };
        let mem = MemoryConfig::resolve(false, false, &config, Some(&remote));
        assert!(mem.initial_injection.enabled);
        assert_eq!(mem.initial_injection.min_score, Some(0.25));
    });
}
#[test]
fn memory_config_local_disabled_blocks_remote_enable() {
    without_memory(|| {
        let config: toml::Value = toml::from_str("[memory]\nenabled = false").unwrap();
        let remote = crate::util::config::RemoteSettings {
            memory_enabled: Some(true),
            ..Default::default()
        };
        let mem = MemoryConfig::resolve(false, false, &config, Some(&remote));
        assert!(
                !mem.enabled,
                "local [memory] enabled=false should block remote enable"
            );
    });
}
#[test]
fn memory_config_local_overrides_remote() {
    without_memory(|| {
        let toml_str = r#"
[memory.search]
max_results = 20
"#;
        let config: toml::Value = toml::from_str(toml_str).unwrap();
        let remote = crate::util::config::RemoteSettings {
            memory_search_max_results: Some(5),
            ..Default::default()
        };
        let mem = MemoryConfig::resolve(false, false, &config, Some(&remote));
        assert_eq!(
                mem.search.max_results, 20,
                "local config should override remote"
            );
    });
}
#[test]
fn memory_config_remote_none_is_noop() {
    without_memory(|| {
        let config = toml::Value::Table(toml::map::Map::new());
        let mem_without = MemoryConfig::resolve(false, false, &config, None);
        let mem_with_empty = MemoryConfig::resolve(
            false,
            false,
            &config,
            Some(&crate::util::config::RemoteSettings::default()),
        );
        assert_eq!(
                mem_without.search.max_results,
                mem_with_empty.search.max_results
            );
        assert_eq!(mem_without.enabled, mem_with_empty.enabled);
    });
}
#[test]
fn flush_semantic_dedup_threshold_from_remote_when_no_local_flush() {
    without_memory(|| {
        let config = toml::Value::Table(toml::map::Map::new());
        let remote = crate::util::config::RemoteSettings {
            flush_semantic_dedup_threshold: Some(0.85),
            ..Default::default()
        };
        let mem = MemoryConfig::resolve(false, false, &config, Some(&remote));
        assert_eq!(
                mem.flush.semantic_dedup_threshold,
                Some(0.85),
                "remote threshold should apply when no local flush config"
            );
    });
}
#[test]
fn flush_semantic_dedup_threshold_clamped_from_remote() {
    without_memory(|| {
        let config = toml::Value::Table(toml::map::Map::new());
        let remote = crate::util::config::RemoteSettings {
            flush_semantic_dedup_threshold: Some(1.5),
            ..Default::default()
        };
        let mem = MemoryConfig::resolve(false, false, &config, Some(&remote));
        assert_eq!(
                mem.flush.semantic_dedup_threshold,
                Some(1.0),
                "remote threshold above 1.0 should be clamped"
            );
        let remote_neg = crate::util::config::RemoteSettings {
            flush_semantic_dedup_threshold: Some(-0.5),
            ..Default::default()
        };
        let mem_neg = MemoryConfig::resolve(false, false, &config, Some(&remote_neg));
        assert_eq!(
                mem_neg.flush.semantic_dedup_threshold,
                Some(0.0),
                "remote threshold below 0.0 should be clamped"
            );
    });
}
#[test]
fn flush_semantic_dedup_threshold_local_blocks_remote() {
    without_memory(|| {
        let toml_str = r#"
[compaction.memory_flush]
enabled = true
semantic_dedup_threshold = 0.88
"#;
        let config: toml::Value = toml::from_str(toml_str).unwrap();
        let remote = crate::util::config::RemoteSettings {
            flush_semantic_dedup_threshold: Some(0.70),
            ..Default::default()
        };
        let mem = MemoryConfig::resolve(false, false, &config, Some(&remote));
        assert_eq!(
                mem.flush.semantic_dedup_threshold,
                Some(0.88),
                "local flush config should block remote override"
            );
    });
}
#[test]
fn flush_semantic_dedup_threshold_defaults_to_none() {
    without_memory(|| {
        let config = toml::Value::Table(toml::map::Map::new());
        let mem = MemoryConfig::resolve(false, false, &config, None);
        assert_eq!(
                mem.flush.semantic_dedup_threshold, None,
                "threshold should default to None (fallback to compiled-in constant)"
            );
    });
}
#[test]
fn memory_dream_config_defaults() {
    without_memory(|| {
        let config = toml::Value::Table(toml::map::Map::new());
        let mem = MemoryConfig::resolve(false, false, &config, None);
        assert!(mem.dream.enabled);
        assert_eq!(mem.dream.min_hours, 4);
        assert_eq!(mem.dream.min_sessions, 3);
        assert_eq!(mem.dream.stale_lock_secs, 3600);
        assert_eq!(mem.dream.check_interval_secs, None);
    });
}
#[test]
fn memory_dream_config_toml_parsing() {
    without_memory(|| {
        let toml_str = r#"
[memory.dream]
enabled = true
min_hours = 12
min_sessions = 3
stale_lock_secs = 1800
check_interval_secs = 600
"#;
        let config: toml::Value = toml::from_str(toml_str).unwrap();
        let mem = MemoryConfig::resolve(false, false, &config, None);
        assert!(mem.dream.enabled);
        assert_eq!(mem.dream.min_hours, 12);
        assert_eq!(mem.dream.min_sessions, 3);
        assert_eq!(mem.dream.stale_lock_secs, 1800);
        assert_eq!(mem.dream.check_interval_secs, Some(600));
    });
}
#[test]
fn memory_dream_config_remote_override_when_toml_absent() {
    without_memory(|| {
        let config = toml::Value::Table(toml::map::Map::new());
        let remote = crate::util::config::RemoteSettings {
            dream_enabled: Some(true),
            dream_min_hours: Some(48),
            dream_min_sessions: Some(10),
            dream_check_interval_secs: Some(900),
            ..Default::default()
        };
        let mem = MemoryConfig::resolve(false, false, &config, Some(&remote));
        assert!(mem.dream.enabled);
        assert_eq!(mem.dream.min_hours, 48);
        assert_eq!(mem.dream.min_sessions, 10);
        assert_eq!(mem.dream.stale_lock_secs, 3600);
        assert_eq!(mem.dream.check_interval_secs, Some(900));
    });
}
#[test]
fn memory_dream_config_remote_ignored_when_toml_present() {
    without_memory(|| {
        let toml_str = r#"
[memory.dream]
enabled = false
min_hours = 6
"#;
        let config: toml::Value = toml::from_str(toml_str).unwrap();
        let remote = crate::util::config::RemoteSettings {
            dream_enabled: Some(true),
            dream_min_hours: Some(48),
            dream_min_sessions: Some(10),
            dream_check_interval_secs: Some(300),
            ..Default::default()
        };
        let mem = MemoryConfig::resolve(false, false, &config, Some(&remote));
        assert!(!mem.dream.enabled, "local TOML should win over remote");
        assert_eq!(mem.dream.min_hours, 6);
        assert_eq!(mem.dream.min_sessions, 3);
        assert_eq!(mem.dream.check_interval_secs, None);
    });
}
#[test]
fn expands_multiple_vars_in_one_string() {
    with_env_var(
        "GROW_TEST_USER",
        "alice",
        || {
            with_env_var(
                "GROW_TEST_ROOT",
                "/a/b/c/d",
                || {
                    let toml_str = r#"
[mcp_servers.test]
command = "$GROW_TEST_USER $GROW_TEST_ROOT"
"#;
                    let mut value = toml::from_str::<toml::Value>(toml_str).unwrap();
                    expand_env_vars_in_toml(&mut value);
                    let toml::Value::Table(table) = value else {
                        panic!("Expected table root");
                    };
                    let Some(toml::Value::Table(mcp_servers)) = table.get("mcp_servers")
                    else {
                        panic!("Expected mcp_servers table");
                    };
                    let Some(toml::Value::Table(test)) = mcp_servers.get("test") else {
                        panic!("Expected test table");
                    };
                    let command = test.get("command").and_then(|v| v.as_str()).unwrap();
                    assert_eq!(command, "alice /a/b/c/d");
                },
            );
        },
    );
}
#[test]
fn effective_half_life_temporal_decay_enabled() {
    let config = MemorySearchConfig {
        temporal_decay: TemporalDecayConfig {
            enabled: true,
            half_life_days: 14.0,
        },
        ..Default::default()
    };
    assert_eq!(config.effective_half_life_days(), Some(14.0));
}
#[test]
fn effective_half_life_temporal_decay_enabled_zero_disables() {
    let config = MemorySearchConfig {
        temporal_decay: TemporalDecayConfig {
            enabled: true,
            half_life_days: 0.0,
        },
        ..Default::default()
    };
    assert_eq!(
            config.effective_half_life_days(),
            None,
            "zero half_life_days should disable decay"
        );
}
#[test]
fn effective_half_life_temporal_decay_enabled_negative_disables() {
    let config = MemorySearchConfig {
        temporal_decay: TemporalDecayConfig {
            enabled: true,
            half_life_days: -5.0,
        },
        ..Default::default()
    };
    assert_eq!(
            config.effective_half_life_days(),
            None,
            "negative half_life_days should disable decay"
        );
}
#[test]
fn effective_half_life_disabled_returns_none() {
    let config = MemorySearchConfig {
        temporal_decay: TemporalDecayConfig {
            enabled: false,
            half_life_days: 30.0,
        },
        ..Default::default()
    };
    assert_eq!(
            config.effective_half_life_days(),
            None,
            "disabled temporal decay should return None"
        );
}
#[test]
fn mmr_lambda_clamped_above_one() {
    without_memory(|| {
        let toml_str = r#"
[memory]
enabled = true

[memory.search.mmr]
enabled = true
lambda = 2.0
"#;
        let config: toml::Value = toml::from_str(toml_str).unwrap();
        let mem = MemoryConfig::resolve(false, false, &config, None);
        assert!(mem.search.mmr.enabled);
        assert!(
                (mem.search.mmr.lambda - 1.0).abs() < f64::EPSILON,
                "lambda=2.0 should clamp to 1.0, got {}",
                mem.search.mmr.lambda
            );
    });
}
#[test]
fn mmr_lambda_clamped_below_zero() {
    without_memory(|| {
        let toml_str = r#"
[memory]
enabled = true

[memory.search.mmr]
enabled = true
lambda = -0.5
"#;
        let config: toml::Value = toml::from_str(toml_str).unwrap();
        let mem = MemoryConfig::resolve(false, false, &config, None);
        assert!(mem.search.mmr.enabled);
        assert!(
                mem.search.mmr.lambda.abs() < f64::EPSILON,
                "lambda=-0.5 should clamp to 0.0, got {}",
                mem.search.mmr.lambda
            );
    });
}
#[test]
fn memory_config_remote_temporal_decay() {
    without_memory(|| {
        let config = toml::Value::Table(toml::map::Map::new());
        let remote = crate::util::config::RemoteSettings {
            memory_temporal_decay_enabled: Some(false),
            memory_temporal_decay_half_life_days: Some(14.0),
            ..Default::default()
        };
        let mem = MemoryConfig::resolve(false, false, &config, Some(&remote));
        assert!(!mem.search.temporal_decay.enabled);
        assert!((mem.search.temporal_decay.half_life_days - 14.0).abs() < f64::EPSILON);
    });
}
#[test]
fn memory_config_remote_mmr() {
    without_memory(|| {
        let config = toml::Value::Table(toml::map::Map::new());
        let remote = crate::util::config::RemoteSettings {
            memory_mmr_enabled: Some(true),
            memory_mmr_lambda: Some(0.5),
            ..Default::default()
        };
        let mem = MemoryConfig::resolve(false, false, &config, Some(&remote));
        assert!(mem.search.mmr.enabled);
        assert!((mem.search.mmr.lambda - 0.5).abs() < f64::EPSILON);
    });
}
#[test]
fn memory_config_remote_mmr_lambda_clamped() {
    without_memory(|| {
        let config = toml::Value::Table(toml::map::Map::new());
        let remote = crate::util::config::RemoteSettings {
            memory_mmr_lambda: Some(5.0),
            ..Default::default()
        };
        let mem = MemoryConfig::resolve(false, false, &config, Some(&remote));
        assert!(
                (mem.search.mmr.lambda - 1.0).abs() < f64::EPSILON,
                "remote mmr_lambda=5.0 should be clamped to 1.0"
            );
    });
}
#[test]
fn memory_config_local_search_blocks_remote_temporal_decay_and_mmr() {
    without_memory(|| {
        let toml_str = r#"
[memory.search]
max_results = 8
"#;
        let config: toml::Value = toml::from_str(toml_str).unwrap();
        let remote = crate::util::config::RemoteSettings {
            memory_temporal_decay_enabled: Some(false),
            memory_mmr_enabled: Some(true),
            memory_mmr_lambda: Some(0.3),
            ..Default::default()
        };
        let mem = MemoryConfig::resolve(false, false, &config, Some(&remote));
        assert!(
                mem.search.temporal_decay.enabled,
                "local search section should block remote temporal_decay override"
            );
        assert!(
                !mem.search.mmr.enabled,
                "local search section should block remote mmr override"
            );
    });
}
/// Mutex to serialize tests that touch the GROW_SUBAGENTS env var.
static SUBAGENTS_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// Run `f` with GROW_SUBAGENTS explicitly unset.
fn without_grow_subagents<T>(f: impl FnOnce() -> T) -> T {
    let _guard = SUBAGENTS_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    with_env_var_opt("GROW_SUBAGENTS", None, f)
}
/// Run `f` with GROW_SUBAGENTS set to a specific value.
fn with_grow_subagents<T>(value: &str, f: impl FnOnce() -> T) -> T {
    let _guard = SUBAGENTS_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    with_env_var_opt("GROW_SUBAGENTS", Some(value), f)
}
#[test]
fn subagents_config_default_enabled() {
    without_grow_subagents(|| {
        let config = toml::Value::Table(toml::map::Map::new());
        let sa = SubagentsConfig::resolve(false, &config);
        assert!(sa.enabled);
    });
}
#[test]
fn subagents_permission_mode_defaults_to_auto() {
    let config: SubagentsConfig = toml::from_str("").unwrap();
    assert_eq!(
        config.permission_mode,
        workspace::permission::types::RequestPermissionMode::Auto
    );
}

#[test]
fn subagents_classifier_input_defaults_to_context_and_parses_request_only() {
    let default: SubagentsConfig = toml::from_str("").unwrap();
    assert_eq!(
        default.classifier_input,
        crate::config::SubagentClassifierInput::Context
    );
    let lean: SubagentsConfig = toml::from_str("classifier_input = \"request_only\"\n").unwrap();
    assert_eq!(
        lean.classifier_input,
        crate::config::SubagentClassifierInput::RequestOnly
    );
    assert!(
        toml::from_str::<SubagentsConfig>("classifier_input = \"full\"\n").is_err(),
        "unknown classifier input modes must fail config parsing"
    );
}
#[test]
fn subagents_permission_modes_parse() {
    use workspace::permission::types::RequestPermissionMode;
    for (raw, expected) in [
        ("ask", RequestPermissionMode::Ask),
        ("auto", RequestPermissionMode::Auto),
        ("always-approve", RequestPermissionMode::AlwaysApprove),
    ] {
        let config: SubagentsConfig =
            toml::from_str(&format!("permission_mode = \"{raw}\"\n")).unwrap();
        assert_eq!(config.permission_mode, expected, "mode={raw}");
    }
}
#[test]
fn subagents_invalid_permission_mode_fails_fast() {
    for invalid in ["silent", "follow"] {
        let error = toml::from_str::<SubagentsConfig>(&format!(
            "permission_mode = \"{invalid}\"\n"
        ))
        .expect_err("unknown and retired permission modes must be rejected");
        assert!(error.to_string().contains("unknown variant"));
        let raw: toml::Value = toml::from_str(&format!(
            "[subagents]\npermission_mode = \"{invalid}\"\n"
        ))
        .unwrap();
        assert!(crate::agent::config::Config::new_from_toml_cfg(&raw).is_err());
    }
}
#[test]
fn subagents_max_depth_defaults_to_one() {
    assert_eq!(
            SubagentsConfig::resolve_max_depth(None, None, None),
            SubagentsConfig::DEFAULT_MAX_DEPTH
        );
    assert_eq!(SubagentsConfig::DEFAULT_MAX_DEPTH, 1);
}
#[test]
fn subagents_max_depth_env_beats_toml_and_remote() {
    assert_eq!(
            SubagentsConfig::resolve_max_depth(Some("3"), Some(2), Some(4)),
            3
        );
}
#[test]
fn subagents_max_depth_toml_beats_remote() {
    assert_eq!(
            SubagentsConfig::resolve_max_depth(None, Some(2), Some(4)),
            2
        );
}
#[test]
fn subagents_max_depth_remote_used_when_local_absent() {
    assert_eq!(SubagentsConfig::resolve_max_depth(None, None, Some(5)), 5);
}
#[test]
fn subagents_max_depth_clamps_below_one_to_one() {
    assert_eq!(SubagentsConfig::clamp_max_depth(-3, "test"), 1);
    assert_eq!(SubagentsConfig::clamp_max_depth(0, "test"), 1);
    assert_eq!(
            SubagentsConfig::resolve_max_depth(Some("-2"), None, None),
            1
        );
    assert_eq!(
            SubagentsConfig::resolve_max_depth(None, Some(0), Some(3)),
            1
        );
    assert_eq!(
            SubagentsConfig::resolve_max_depth(None, None, Some(0)),
            1
        );
}
#[test]
fn subagents_max_depth_invalid_env_falls_through() {
    assert_eq!(
            SubagentsConfig::resolve_max_depth(Some("not-a-number"), Some(2), None),
            2
        );
}
#[test]
fn subagents_config_parses_max_depth_from_toml() {
    without_grow_subagents(|| {
        let config: toml::Value = toml::from_str("[subagents]\nmax_depth = 2\n")
            .unwrap();
        let sa = SubagentsConfig::resolve(false, &config);
        assert_eq!(sa.max_depth, Some(2));
    });
}
#[test]
fn subagents_config_parses_negative_max_depth_without_dropping_section() {
    without_grow_subagents(|| {
        let config: toml::Value = toml::from_str(
                "[subagents]\nenabled = true\nmax_depth = -1\n",
            )
            .unwrap();
        let sa = SubagentsConfig::resolve(false, &config);
        assert!(sa.enabled);
        assert_eq!(sa.max_depth, Some(-1));
        assert_eq!(
                SubagentsConfig::resolve_max_depth(None, sa.max_depth, None),
                1
            );
    });
}
#[test]
fn subagents_config_cli_flag_enables() {
    without_grow_subagents(|| {
        let config = toml::Value::Table(toml::map::Map::new());
        let sa = SubagentsConfig::resolve(true, &config);
        assert!(sa.enabled);
    });
}
#[test]
fn subagents_config_env_var_enables() {
    with_grow_subagents(
        "1",
        || {
            let config = toml::Value::Table(toml::map::Map::new());
            let sa = SubagentsConfig::resolve(false, &config);
            assert!(sa.enabled);
        },
    );
}
#[test]
fn subagents_config_env_var_disables() {
    with_grow_subagents(
        "0",
        || {
            let config: toml::Value = toml::from_str("[subagents]\nenabled = true")
                .unwrap();
            let sa = SubagentsConfig::resolve(false, &config);
            assert!(!sa.enabled, "GROW_SUBAGENTS=0 should override config file");
        },
    );
}
#[test]
fn subagents_config_toml_enables() {
    without_grow_subagents(|| {
        let config: toml::Value = toml::from_str("[subagents]\nenabled = true").unwrap();
        let sa = SubagentsConfig::resolve(false, &config);
        assert!(sa.enabled);
    });
}
#[test]
fn subagents_config_local_disabled_wins() {
    without_grow_subagents(|| {
        let config: toml::Value = toml::from_str("[subagents]\nenabled = false")
            .unwrap();
        let sa = SubagentsConfig::resolve(false, &config);
        assert!(!sa.enabled, "local [subagents] enabled=false should win");
    });
}
#[test]
fn subagents_config_env_var_disables_default() {
    with_grow_subagents(
        "0",
        || {
            let config = toml::Value::Table(toml::map::Map::new());
            let sa = SubagentsConfig::resolve(false, &config);
            assert!(
                !sa.enabled,
                "GROW_SUBAGENTS=0 should override the enabled default"
            );
        },
    );
}
/// A `subagents_enabled` key served by an old cli-chat-proxy must parse
/// as an unknown key and have no effect on resolution.
#[test]
fn subagents_config_remote_settings_key_is_ignored() {
    without_grow_subagents(|| {
        let _settings: crate::util::config::RemoteSettings = serde_json::from_str(
                r#"{"subagents_enabled": false}"#,
            )
            .expect("unknown subagents_enabled key must not break parsing");
        let config = toml::Value::Table(toml::map::Map::new());
        let sa = SubagentsConfig::resolve(false, &config);
        assert!(sa.enabled);
    });
}
#[test]
fn subagents_config_cli_flag_overrides_env_var() {
    with_grow_subagents(
        "0",
        || {
            let config = toml::Value::Table(toml::map::Map::new());
            let sa = SubagentsConfig::resolve(true, &config);
            assert!(
                sa.enabled,
                "--subagents CLI flag should override GROW_SUBAGENTS=0"
            );
        },
    );
}
#[test]
fn subagents_config_models_parsed() {
    without_grow_subagents(|| {
        let config: toml::Value = toml::from_str(
                r#"
                [subagents]
                enabled = true

                [subagents.models]
                explore = "grow-3-fast"
                plan = "grow-4.5"
                "#,
            )
            .unwrap();
        let sa = SubagentsConfig::resolve(false, &config);
        assert!(sa.enabled);
        assert_eq!(sa.models.len(), 2);
        assert_eq!(sa.models.get("explore").unwrap(), "grow-3-fast");
        assert_eq!(sa.models.get("plan").unwrap(), "grow-4.5");
    });
}
#[test]
fn subagents_config_models_empty_when_missing() {
    without_grow_subagents(|| {
        let config: toml::Value = toml::from_str("[subagents]\nenabled = true").unwrap();
        let sa = SubagentsConfig::resolve(false, &config);
        assert!(sa.enabled);
        assert!(sa.models.is_empty());
    });
}
#[test]
fn subagents_config_models_without_enabled() {
    without_grow_subagents(|| {
        let config: toml::Value = toml::from_str(
                r#"
                [subagents.models]
                explore = "grow-3-fast"
                "#,
            )
            .unwrap();
        let sa = SubagentsConfig::resolve(false, &config);
        assert!(
                !sa.enabled,
                "explicit [subagents] section without enabled should be false"
            );
        assert_eq!(sa.models.len(), 1);
        assert_eq!(sa.models.get("explore").unwrap(), "grow-3-fast");
    });
}
#[test]
fn subagents_config_models_with_env_var_enables() {
    with_grow_subagents(
        "1",
        || {
            let config: toml::Value = toml::from_str(
                    r#"
                [subagents.models]
                explore = "grow-3-fast"
                "#,
                )
                .unwrap();
            let sa = SubagentsConfig::resolve(false, &config);
            assert!(sa.enabled, "GROW_SUBAGENTS=1 should enable");
            assert_eq!(sa.models.get("explore").unwrap(), "grow-3-fast");
        },
    );
}
#[test]
fn subagents_config_toggle_mixed_values() {
    without_grow_subagents(|| {
        let config: toml::Value = toml::from_str(
                r#"
                [subagents]
                enabled = true

                [subagents.toggle]
                explore = true
                plan = false
                general-purpose = true
                code-reviewer = false
                "#,
            )
            .unwrap();
        let sa = SubagentsConfig::resolve(false, &config);
        assert!(sa.enabled);
        assert_eq!(sa.toggle.len(), 4);
        assert_eq!(sa.toggle.get("explore").copied(), Some(true));
        assert_eq!(sa.toggle.get("plan").copied(), Some(false));
        assert_eq!(sa.toggle.get("general-purpose").copied(), Some(true));
        assert_eq!(sa.toggle.get("code-reviewer").copied(), Some(false));
    });
}
#[test]
fn subagents_config_toggle_missing_defaults_to_empty() {
    without_grow_subagents(|| {
        let config: toml::Value = toml::from_str("[subagents]\nenabled = true").unwrap();
        let sa = SubagentsConfig::resolve(false, &config);
        assert!(sa.enabled);
        assert!(
                sa.toggle.is_empty(),
                "missing [subagents.toggle] should produce empty HashMap"
            );
    });
}
#[test]
fn subagents_config_is_subagent_enabled_absent_defaults_true() {
    let sa = SubagentsConfig {
        enabled: true,
        toggle: std::collections::HashMap::from([("plan".to_string(), false)]),
        ..Default::default()
    };
    assert!(
            sa.is_subagent_enabled("explore"),
            "absent key should default to enabled (true)"
        );
    assert!(
            sa.is_subagent_enabled("general-purpose"),
            "absent key should default to enabled (true)"
        );
}
#[test]
fn subagents_config_is_subagent_enabled_false_when_toggled_off() {
    let sa = SubagentsConfig {
        enabled: true,
        toggle: std::collections::HashMap::from([
            ("plan".to_string(), false),
            ("code-reviewer".to_string(), false),
            ("explore".to_string(), true),
        ]),
        ..Default::default()
    };
    assert!(
            !sa.is_subagent_enabled("plan"),
            "plan = false should return disabled"
        );
    assert!(
            !sa.is_subagent_enabled("code-reviewer"),
            "code-reviewer = false should return disabled"
        );
    assert!(
            sa.is_subagent_enabled("explore"),
            "explore = true should return enabled"
        );
}
fn with_model_overrides_env_full<T>(
    title: Option<&str>,
    id: Option<&str>,
    ps: Option<&str>,
    f: impl FnOnce() -> T,
) -> T {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    with_env_var_opt(
        "GROW_SESSION_TITLE_MODEL",
        title,
        || with_env_var_opt(
            "GROW_IMAGE_DESCRIPTION_MODEL",
            id,
            || with_env_var_opt("GROW_PROMPT_SUGGESTIONS_MODEL", ps, f),
        ),
    )
}
fn with_model_overrides_env<T>(
    title: Option<&str>,
    id: Option<&str>,
    f: impl FnOnce() -> T,
) -> T {
    with_model_overrides_env_full(title, id, None, f)
}
#[test]
fn model_overrides_remote_settings_applies_without_local_config() {
    with_model_overrides_env(
        None,
        None,
        || {
            let empty = toml::Value::Table(toml::map::Map::new());
            let remote = crate::util::config::RemoteSettings {
                session_title_model: Some("remote-ss".to_owned()),
                image_description_model: Some("remote-id".to_owned()),
                ..Default::default()
            };
            let cfg = ModelOverrideConfig::resolve(None, &empty, Some(&remote));
            assert_eq!(cfg.session_title, Some("remote-ss".to_owned()));
            assert_eq!(cfg.image_description, Some("remote-id".to_owned()));
        },
    );
}
#[test]
fn model_overrides_local_image_description_wins_over_remote() {
    with_model_overrides_env(
        None,
        None,
        || {
            let config: toml::Value = toml::from_str(
                    r#"
                [models]
                image_description = "local-id"
                "#,
                )
                .unwrap();
            let remote = crate::util::config::RemoteSettings {
                image_description_model: Some("remote-id".to_owned()),
                ..Default::default()
            };
            let cfg = ModelOverrideConfig::resolve(None, &config, Some(&remote));
            assert_eq!(cfg.image_description, Some("local-id".to_owned()));
        },
    );
}
#[test]
fn model_overrides_default_image_description_is_unconfigured() {
    with_model_overrides_env(
        None,
        None,
        || {
            let empty = toml::Value::Table(toml::map::Map::new());
            let cfg = ModelOverrideConfig::resolve(None, &empty, None);
            assert_eq!(cfg.image_description, None);
        },
    );
}
#[test]
fn model_overrides_default_session_title_uses_active_model() {
    with_model_overrides_env(
        None,
        None,
        || {
            let empty = toml::Value::Table(toml::map::Map::new());
            let cfg = ModelOverrideConfig::resolve(None, &empty, None);
            assert_eq!(cfg.session_title, None);
        },
    );
}
#[test]
fn model_overrides_local_session_title_wins_over_remote() {
    with_model_overrides_env(
        None,
        None,
        || {
            let config: toml::Value = toml::from_str(
                    r#"
                [models]
                session_title = "local-ss"
                "#,
                )
                .unwrap();
            let remote = crate::util::config::RemoteSettings {
                session_title_model: Some("remote-ss".to_owned()),
                ..Default::default()
            };
            let cfg = ModelOverrideConfig::resolve(None, &config, Some(&remote));
            assert_eq!(cfg.session_title, Some("local-ss".to_owned()));
        },
    );
}
#[test]
fn model_overrides_env_session_title_overrides_remote() {
    with_model_overrides_env(
        Some("env-ss"),
        None,
        || {
            let empty = toml::Value::Table(toml::map::Map::new());
            let remote = crate::util::config::RemoteSettings {
                session_title_model: Some("remote-ss".to_owned()),
                ..Default::default()
            };
            let cfg = ModelOverrideConfig::resolve(None, &empty, Some(&remote));
            assert_eq!(cfg.session_title, Some("env-ss".to_owned()));
        },
    );
}
#[test]
fn model_overrides_env_session_title_overrides_local() {
    with_model_overrides_env(
        Some("env-ss"),
        None,
        || {
            let config: toml::Value = toml::from_str(
                    r#"
                [models]
                session_title = "local-ss"
                "#,
                )
                .unwrap();
            let cfg = ModelOverrideConfig::resolve(None, &config, None);
            assert_eq!(cfg.session_title, Some("env-ss".to_owned()));
        },
    );
}
#[test]
fn model_overrides_empty_session_title_toml_uses_active_model() {
    with_model_overrides_env(
        None,
        None,
        || {
            let config: toml::Value = toml::from_str(
                    r#"
                [models]
                session_title = ""
                "#,
                )
                .unwrap();
            let cfg = ModelOverrideConfig::resolve(None, &config, None);
            assert_eq!(cfg.session_title, None);
        },
    );
}
#[test]
fn model_overrides_empty_session_title_remote_uses_active_model() {
    with_model_overrides_env(
        None,
        None,
        || {
            let empty = toml::Value::Table(toml::map::Map::new());
            let remote = crate::util::config::RemoteSettings {
                session_title_model: Some("   ".to_owned()),
                ..Default::default()
            };
            let cfg = ModelOverrideConfig::resolve(None, &empty, Some(&remote));
            assert_eq!(cfg.session_title, None);
        },
    );
}
#[test]
fn model_overrides_cli_session_title_overrides_everything() {
    with_model_overrides_env(
        Some("env-ss"),
        None,
        || {
            let config: toml::Value = toml::from_str(
                    r#"
                [models]
                session_title = "local-ss"
                "#,
                )
                .unwrap();
            let remote = crate::util::config::RemoteSettings {
                session_title_model: Some("remote-ss".to_owned()),
                ..Default::default()
            };
            let cfg = ModelOverrideConfig::resolve(
                Some("cli-ss"),
                &config,
                Some(&remote),
            );
            assert_eq!(cfg.session_title, Some("cli-ss".to_owned()));
        },
    );
}
#[test]
fn model_overrides_empty_cli_session_title_uses_active_model() {
    with_model_overrides_env(
        None,
        None,
        || {
            let empty = toml::Value::Table(toml::map::Map::new());
            let cfg = ModelOverrideConfig::resolve(Some(""), &empty, None);
            assert_eq!(cfg.session_title, None);
        },
    );
}
#[test]
fn model_overrides_env_image_description_overrides_remote() {
    with_model_overrides_env(
        None,
        Some("env-id"),
        || {
            let empty = toml::Value::Table(toml::map::Map::new());
            let remote = crate::util::config::RemoteSettings {
                image_description_model: Some("remote-id".to_owned()),
                ..Default::default()
            };
            let cfg = ModelOverrideConfig::resolve(None, &empty, Some(&remote));
            assert_eq!(cfg.image_description, Some("env-id".to_owned()));
        },
    );
}
#[test]
fn model_overrides_env_image_description_overrides_local() {
    with_model_overrides_env(
        None,
        Some("env-id"),
        || {
            let config: toml::Value = toml::from_str(
                    r#"
                [models]
                image_description = "local-id"
                "#,
                )
                .unwrap();
            let cfg = ModelOverrideConfig::resolve(None, &config, None);
            assert_eq!(cfg.image_description, Some("env-id".to_owned()));
        },
    );
}
#[test]
fn model_overrides_empty_image_description_toml_is_unconfigured() {
    with_model_overrides_env(
        None,
        None,
        || {
            let config: toml::Value = toml::from_str(
                    r#"
                [models]
                image_description = ""
                "#,
                )
                .unwrap();
            let cfg = ModelOverrideConfig::resolve(None, &config, None);
            assert_eq!(cfg.image_description, None);
        },
    );
}
#[test]
fn model_overrides_empty_image_description_remote_is_unconfigured() {
    with_model_overrides_env(
        None,
        None,
        || {
            let empty = toml::Value::Table(toml::map::Map::new());
            let remote = crate::util::config::RemoteSettings {
                image_description_model: Some("   ".to_owned()),
                ..Default::default()
            };
            let cfg = ModelOverrideConfig::resolve(None, &empty, Some(&remote));
            assert_eq!(cfg.image_description, None);
        },
    );
}
#[test]
fn model_overrides_prompt_suggestion_unpinned_by_default() {
    with_model_overrides_env(
        None,
        None,
        || {
            let empty = toml::Value::Table(toml::map::Map::new());
            let cfg = ModelOverrideConfig::resolve(None, &empty, None);
            assert_eq!(cfg.prompt_suggestion, PromptSuggestModelPin::Unpinned);
        },
    );
}
#[test]
fn model_overrides_prompt_suggestion_local_wins_over_remote() {
    with_model_overrides_env(
        None,
        None,
        || {
            let config: toml::Value = toml::from_str(
                    r#"
                [models]
                prompt_suggestion = "local-ps"
                "#,
                )
                .unwrap();
            let remote = crate::util::config::RemoteSettings {
                prompt_suggestion_model: Some("remote-ps".to_owned()),
                ..Default::default()
            };
            let cfg = ModelOverrideConfig::resolve(None, &config, Some(&remote));
            assert_eq!(
                cfg.prompt_suggestion,
                PromptSuggestModelPin::Pinned("local-ps".to_owned())
            );
        },
    );
}
#[test]
fn model_overrides_prompt_suggestion_remote_applies_without_local() {
    with_model_overrides_env(
        None,
        None,
        || {
            let empty = toml::Value::Table(toml::map::Map::new());
            let remote = crate::util::config::RemoteSettings {
                prompt_suggestion_model: Some("remote-ps".to_owned()),
                ..Default::default()
            };
            let cfg = ModelOverrideConfig::resolve(None, &empty, Some(&remote));
            assert_eq!(
                cfg.prompt_suggestion,
                PromptSuggestModelPin::Pinned("remote-ps".to_owned())
            );
        },
    );
}
#[test]
fn model_overrides_prompt_suggestion_env_wins_over_local_and_remote() {
    with_model_overrides_env_full(
        None,
        None,
        Some("env-ps"),
        || {
            let config: toml::Value = toml::from_str(
                    r#"
                [models]
                prompt_suggestion = "local-ps"
                "#,
                )
                .unwrap();
            let remote = crate::util::config::RemoteSettings {
                prompt_suggestion_model: Some("remote-ps".to_owned()),
                ..Default::default()
            };
            let cfg = ModelOverrideConfig::resolve(None, &config, Some(&remote));
            assert_eq!(
                cfg.prompt_suggestion,
                PromptSuggestModelPin::Env("env-ps".to_owned())
            );
        },
    );
}
#[test]
fn model_overrides_prompt_suggestion_blank_values_are_unset() {
    with_model_overrides_env_full(
        None,
        None,
        Some("  "),
        || {
            let config: toml::Value = toml::from_str(
                    r#"
                [models]
                prompt_suggestion = "local-ps"
                "#,
                )
                .unwrap();
            let cfg = ModelOverrideConfig::resolve(None, &config, None);
            assert_eq!(
                cfg.prompt_suggestion,
                PromptSuggestModelPin::Pinned("local-ps".to_owned())
            );
        },
    );
    with_model_overrides_env(
        None,
        None,
        || {
            let config: toml::Value = toml::from_str(
                    r#"
                [models]
                prompt_suggestion = "   "
                "#,
                )
                .unwrap();
            let remote = crate::util::config::RemoteSettings {
                prompt_suggestion_model: Some("  ".to_owned()),
                ..Default::default()
            };
            let cfg = ModelOverrideConfig::resolve(None, &config, Some(&remote));
            assert_eq!(cfg.prompt_suggestion, PromptSuggestModelPin::Unpinned);
        },
    );
}
/// Lock shared by every test that touches the env var read by
/// `ToolsConfig::resolve`.
static TOOLS_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// Set the `ToolsConfig` env var for the duration of `f`, then restore.
fn with_tools_env<T>(respect_gitignore: Option<&str>, f: impl FnOnce() -> T) -> T {
    let _guard = TOOLS_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    with_env_var_opt("GROW_RESPECT_GITIGNORE", respect_gitignore, f)
}
fn without_grow_respect_gitignore<T>(f: impl FnOnce() -> T) -> T {
    with_tools_env(None, f)
}
fn with_grow_respect_gitignore<T>(value: &str, f: impl FnOnce() -> T) -> T {
    with_tools_env(Some(value), f)
}
#[test]
fn tools_config_default_disabled() {
    without_grow_respect_gitignore(|| {
        let config = toml::Value::Table(toml::map::Map::new());
        let tc = ToolsConfig::resolve(&config);
        assert!(!tc.respect_gitignore);
    });
}
#[test]
fn tools_config_toml_disables() {
    without_grow_respect_gitignore(|| {
        let config: toml::Value = toml::from_str("[tools]\nrespect_gitignore = false")
            .unwrap();
        let tc = ToolsConfig::resolve(&config);
        assert!(!tc.respect_gitignore);
    });
}
#[test]
fn tools_config_env_var_disables() {
    with_grow_respect_gitignore(
        "0",
        || {
            let config = toml::Value::Table(toml::map::Map::new());
            let tc = ToolsConfig::resolve(&config);
            assert!(!tc.respect_gitignore);
        },
    );
}
#[test]
fn tools_config_env_var_overrides_toml() {
    with_grow_respect_gitignore(
        "1",
        || {
            let config: toml::Value = toml::from_str(
                    "[tools]\nrespect_gitignore = false",
                )
                .unwrap();
            let tc = ToolsConfig::resolve(&config);
            assert!(tc.respect_gitignore, "env var should override config file");
        },
    );
}
#[test]
fn tools_config_env_false_overrides_toml_true() {
    with_grow_respect_gitignore(
        "false",
        || {
            let config: toml::Value = toml::from_str("[tools]\nrespect_gitignore = true")
                .unwrap();
            let tc = ToolsConfig::resolve(&config);
            assert!(
                !tc.respect_gitignore,
                "GROW_RESPECT_GITIGNORE=false should override config file"
            );
        },
    );
}
#[test]
fn removed_role_config_is_rejected() {
    let error = toml::from_str::<SubagentsConfig>(
        r#"
        [roles.researcher]
        description = "old role"
        "#,
    )
    .unwrap_err();
    assert!(error.to_string().contains("unknown field `roles`"));
}
#[test]
fn add_hooks_path_appends() {
    let tmp = tempfile::tempdir().unwrap();
    let paths_file = tmp.path().join("hooks-paths");
    let _ = add_hooks_path_to_file("/some/path", &paths_file);
    let content = std::fs::read_to_string(&paths_file).unwrap_or_default();
    assert!(content.contains("/some/path"));
}
#[test]
fn add_hooks_path_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let paths_file = tmp.path().join("hooks-paths");
    let _ = add_hooks_path_to_file("/dup/path", &paths_file);
    let _ = add_hooks_path_to_file("/dup/path", &paths_file);
    let content = std::fs::read_to_string(&paths_file).unwrap_or_default();
    let count = content.lines().filter(|l| l.trim() == "/dup/path").count();
    assert_eq!(count, 1);
}
#[test]
fn remove_hooks_path_removes() {
    let tmp = tempfile::tempdir().unwrap();
    let paths_file = tmp.path().join("hooks-paths");
    let _ = add_hooks_path_to_file("/to/remove", &paths_file);
    let _ = remove_hooks_path_from_file("/to/remove", &paths_file);
    let content = std::fs::read_to_string(&paths_file).unwrap_or_default();
    assert!(!content.contains("/to/remove"));
}
#[test]
fn remove_hooks_path_is_noop_if_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let paths_file = tmp.path().join("hooks-paths");
    let result = remove_hooks_path_from_file("/nonexistent/path", &paths_file);
    assert!(result.is_ok());
}
#[test]
fn remove_hooks_path_preserves_others() {
    let tmp = tempfile::tempdir().unwrap();
    let paths_file = tmp.path().join("hooks-paths");
    let _ = add_hooks_path_to_file("/keep/me", &paths_file);
    let _ = add_hooks_path_to_file("/remove/me", &paths_file);
    let _ = add_hooks_path_to_file("/keep/me/too", &paths_file);
    let _ = remove_hooks_path_from_file("/remove/me", &paths_file);
    let content = std::fs::read_to_string(&paths_file).unwrap_or_default();
    assert!(content.contains("/keep/me"));
    assert!(content.contains("/keep/me/too"));
    assert!(!content.contains("/remove/me"));
}
#[test]
fn add_hooks_path_succeeds_on_first_add() {
    let tmp = tempfile::tempdir().unwrap();
    let paths_file = tmp.path().join("hooks-paths");
    let result = add_hooks_path_to_file("/first", &paths_file);
    assert!(result.is_ok());
}
#[test]
fn add_hooks_path_succeeds_on_duplicate() {
    let tmp = tempfile::tempdir().unwrap();
    let paths_file = tmp.path().join("hooks-paths");
    let _ = add_hooks_path_to_file("/dup", &paths_file);
    let result = add_hooks_path_to_file("/dup", &paths_file);
    assert!(result.is_ok());
}
#[test]
fn remove_hooks_path_succeeds_when_present() {
    let tmp = tempfile::tempdir().unwrap();
    let paths_file = tmp.path().join("hooks-paths");
    let _ = add_hooks_path_to_file("/present", &paths_file);
    let result = remove_hooks_path_from_file("/present", &paths_file);
    assert!(result.is_ok());
}
#[test]
fn remove_hooks_path_succeeds_when_absent() {
    let tmp = tempfile::tempdir().unwrap();
    let paths_file = tmp.path().join("hooks-paths");
    let result = remove_hooks_path_from_file("/missing", &paths_file);
    assert!(result.is_ok());
}
#[test]
fn add_dismissed_plugin_cta_creates_table() {
    let tmp = tempfile::tempdir().unwrap();
    let config_path = tmp.path().join("config.toml");
    add_dismissed_plugin_cta_to_file("figma", &config_path).unwrap();
    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("[plugin_cta]"));
    assert!(content.contains("figma"));
    assert!(dismissed_plugin_ctas_in_file(&config_path).contains("figma"));
}
#[test]
fn add_dismissed_plugin_cta_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let config_path = tmp.path().join("config.toml");
    add_dismissed_plugin_cta_to_file("notion", &config_path).unwrap();
    add_dismissed_plugin_cta_to_file("notion", &config_path).unwrap();
    let config: toml::Value = toml::from_str(
            &std::fs::read_to_string(&config_path).unwrap(),
        )
        .unwrap();
    let dismissed = config
        .get("plugin_cta")
        .and_then(|v| v.get("dismissed"))
        .and_then(|v| v.as_array())
        .unwrap();
    let count = dismissed.iter().filter(|v| v.as_str() == Some("notion")).count();
    assert_eq!(count, 1);
}
#[test]
fn dismissed_plugin_ctas_reflects_added_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let config_path = tmp.path().join("config.toml");
    assert!(!dismissed_plugin_ctas_in_file(&config_path).contains("figma"));
    add_dismissed_plugin_cta_to_file("figma", &config_path).unwrap();
    let dismissed = dismissed_plugin_ctas_in_file(&config_path);
    assert!(dismissed.contains("figma"));
    assert!(!dismissed.contains("notion"));
}
#[test]
fn add_dismissed_plugin_cta_preserves_other_config() {
    let tmp = tempfile::tempdir().unwrap();
    let config_path = tmp.path().join("config.toml");
    std::fs::write(&config_path, "[plugins]\ndisabled = [\"keep-me\"]\n").unwrap();
    add_dismissed_plugin_cta_to_file("figma", &config_path).unwrap();
    let config: toml::Value = toml::from_str(
            &std::fs::read_to_string(&config_path).unwrap(),
        )
        .unwrap();
    assert_eq!(
            config
                .get("plugins")
                .and_then(|v| v.get("disabled"))
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|v| v.as_str()),
            Some("keep-me"),
        );
    assert!(dismissed_plugin_ctas_in_file(&config_path).contains("figma"));
}
/// A provider in a trusted disk layer resolves through the real
/// `ConfigLayers` → `effective_config_disk_only` → parse seam that the
/// direct-TOML parse tests bypass. (`ConfigLayers` has no project slot, so
/// a repo `.grow/config.toml` structurally cannot supply one.)
#[test]
fn auth_provider_honored_only_from_trusted_disk_layers() {
    let layers = ConfigLayers {
        user: toml::from_str(
                "[auth_provider.corp]\ncommand = \"/usr/local/bin/corp-token\"\n",
            )
            .unwrap(),
        ..Default::default()
    };
    let cfg = crate::agent::config::Config::new_from_toml_cfg(
            &layers.effective_config_disk_only(),
        )
        .unwrap();
    assert_eq!(
            cfg.auth_providers.get("corp").map(|c| c.command.as_str()),
            Some("/usr/local/bin/corp-token"),
            "a provider in a trusted disk layer is honored"
        );
}
#[test]
fn provider_catalog_honored_only_from_trusted_disk_layers() {
    let layers = ConfigLayers {
        user: toml::from_str(
                "[provider.gateway]\napi_backend = \"responses\"\n\
                 [provider.gateway.options]\nbase_url = \"https://gateway.example/v1\"\n\
                 [provider.gateway.options.auth]\ntype = \"command\"\ncommand = \"/usr/local/bin/gw-token\"\n\
                 [provider.gateway.models.gateway-model]\nname = \"Gateway Model\"\n",
            )
            .unwrap(),
        ..Default::default()
    };
    let cfg = crate::agent::config::Config::new_from_toml_cfg(
            &layers.effective_config_disk_only(),
        )
        .unwrap();
    assert!(
            cfg.config_models.contains_key("gateway/gateway-model"),
            "a provider model in a trusted disk layer is honored"
        );
    assert_eq!(
            cfg.auth_providers
                .get("provider:gateway")
                .map(|c| c.command.as_str()),
            Some("/usr/local/bin/gw-token"),
            "its inline auth registers as a synthetic auth provider"
        );
}
#[test]
fn config_layers_origins_tracks_source() {
    use crate::agent::config::ConfigSource;
    let layers = ConfigLayers {
        user: toml::from_str("[features]\nlsp_tools = false\n[ui]\ntheme = \"dark\"\n")
            .unwrap(),
        ..Default::default()
    };
    let origins = config_origins(&layers);
    assert_eq!(origins["features.lsp_tools"], ConfigSource::Config);
    assert_eq!(origins["ui.theme"], ConfigSource::Config);
}
#[test]
fn validate_hooks_path_rejects_relative_path() {
    let result = validate_hooks_path("relative/path/hooks");
    assert!(result.is_err());
    assert!(
            result.unwrap_err().to_string().contains("absolute"),
            "should mention 'absolute'"
        );
}
#[test]
fn validate_hooks_path_rejects_outside_grow_home() {
    let result = validate_hooks_path("/tmp/evil-hooks");
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
            msg.contains("must be under ~/.grow/"),
            "should mention ~/.grow/ restriction, got: {msg}"
        );
}
#[test]
fn validate_hooks_path_rejects_traversal_attack() {
    let grow_home = crate::util::grow_home::grow_home();
    let traversal = format!("{}/../evil", grow_home.display());
    let result = validate_hooks_path(&traversal);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
            msg.contains("must be under ~/.grow/"),
            "traversal should be rejected, got: {msg}"
        );
}
#[test]
fn validate_hooks_path_accepts_hooks_subdir() {
    let grow_home = crate::util::grow_home::grow_home();
    let valid_path = grow_home.join("hooks").join("my-hooks");
    let _ = std::fs::create_dir_all(&valid_path);
    let result = validate_hooks_path(valid_path.to_str().unwrap());
    assert!(result.is_ok(), "path under ~/.grow/ should be accepted");
}
/// Simulate a release-stamped build so the folder-trust gate engages (a
/// local/dev build auto-trusts). Hold the returned guard for the test body.
fn simulate_release_build() -> test_support::EnvGuard {
    test_support::EnvGuard::set(version::TEST_VERSION_ENV, "0.0.0-sim")
}
/// SECURITY (plugin-RCE): a PROJECT-declared `[plugins].paths` loads as an
/// auto-enabled, auto-trusted ConfigPath plugin, so it must merge into the
/// effective config ONLY when the folder is trusted; project
/// `[plugins].disabled` is never gated. The closing set-difference proves
/// the gate toggles ONLY that path (user/global paths pass through both
/// verdicts untouched). GROW_HOME-isolated + `#[serial]` for folder-trust
/// store hygiene (empty store ⇒ deterministic untrusted;
/// `EnvGuard` restores GROW_HOME even on panic). No user-global
/// `$GROW_HOME/config.toml` is seeded: `grow_home()` is `OnceLock`-cached,
/// so under a shared-process harness (Bazel) such a seed is read
/// non-deterministically — reliable only under nextest's process-per-test
/// isolation.
#[test]
#[serial_test::serial]
fn resolve_effective_plugins_config_gates_project_paths_on_folder_trust() {
    use test_support::EnvGuard;
    let home = tempfile::tempdir().unwrap();
    let _env = EnvGuard::set("GROW_HOME", home.path());
    let _flag = EnvGuard::unset("GROW_FOLDER_TRUST");
    let _sim = simulate_release_build();
    let repo = tempfile::tempdir().unwrap();
    git2::Repository::init(repo.path()).unwrap();
    let grow = repo.path().join(".grow");
    std::fs::create_dir_all(&grow).unwrap();
    std::fs::write(
            grow.join("config.toml"),
            "[plugins]\npaths = [\"./proj-plugin\"]\ndisabled = [\"proj-bad\"]\n",
        )
        .unwrap();
    let cwd = repo.path();
    let proj_path = "./proj-plugin".to_string();
    let proj_disabled = "proj-bad".to_string();
    let untrusted = resolve_effective_plugins_config(cwd);
    assert!(
            !untrusted.paths.contains(&proj_path),
            "untrusted folder must NOT merge the project [plugins].paths"
        );
    assert!(
            untrusted.disabled.contains(&proj_disabled),
            "project [plugins].disabled must merge even when untrusted (fail-safe)"
        );
    crate::agent::folder_trust::grant_folder_trust(cwd);
    let trusted = resolve_effective_plugins_config(cwd);
    assert!(
            trusted.paths.contains(&proj_path),
            "trusted folder must merge the project [plugins].paths"
        );
    assert!(
            trusted.disabled.contains(&proj_disabled),
            "project [plugins].disabled must merge when trusted too"
        );
    let trusted_minus_project: Vec<String> = trusted
        .paths
        .iter()
        .filter(|p| *p != &proj_path)
        .cloned()
        .collect();
    assert_eq!(
            trusted_minus_project, untrusted.paths,
            "the trust gate must toggle ONLY the project path; user/global paths unaffected"
        );
}
/// SECURITY (plugin-RCE) end-to-end: prove through the REAL `discover_plugins`
/// that a PROJECT-declared `[plugins].paths` ConfigPath plugin is EXCLUDED
/// from discovery while untrusted and included once trusted. The Part-2
/// set-difference test covers the config merge; this closes the loop at the
/// discovery boundary (if it is never discovered it can never activate).
/// Mirrors the Project-scope analog `discover_real_project_plugin_gated_on_project_trusted`
/// in `agent`. An ABSOLUTE plugin path is used so the merged
/// `config_paths` entry resolves against the repo — `discover_plugins`' `is_dir()`
/// check resolves a relative `./x` against the process cwd, not `cwd`.
/// GROW_HOME-isolated + `#[serial]` (`EnvGuard` restores it even on panic).
#[test]
#[serial_test::serial]
fn discover_plugins_excludes_untrusted_configpath_plugin_end_to_end() {
    use agent::plugins::{TrustStore, discover_plugins};
    use test_support::EnvGuard;
    let home = tempfile::tempdir().unwrap();
    let _env = EnvGuard::set("GROW_HOME", home.path());
    let _flag = EnvGuard::unset("GROW_FOLDER_TRUST");
    let _sim = simulate_release_build();
    let repo = tempfile::tempdir().unwrap();
    git2::Repository::init(repo.path()).unwrap();
    let cwd = repo.path();
    let plugin_dir = cwd.join("cfgpath-probe");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(plugin_dir.join("plugin.json"), r#"{"name":"cfgpath-probe"}"#)
        .unwrap();
    let grow = cwd.join(".grow");
    std::fs::create_dir_all(&grow).unwrap();
    std::fs::write(
            grow.join("config.toml"),
            format!("[plugins]\npaths = ['{}']\n", plugin_dir.display()),
        )
        .unwrap();
    let trust_store = TrustStore::load_from(home.path().join("plugin-trust"));
    let untrusted_dc = resolve_effective_plugins_config(cwd).to_discovery_config();
    let untrusted_verdict = crate::agent::folder_trust::project_scope_allowed(cwd);
    assert!(
            !untrusted_verdict,
            "a fresh repo declaring [plugins].paths must resolve untrusted"
        );
    assert!(
            !untrusted_dc
                .config_paths
                .iter()
                .any(|p| p.ends_with("cfgpath-probe")),
            "untrusted: the project path must be absent from config_paths"
        );
    let untrusted_found = discover_plugins(
            Some(cwd),
            &untrusted_dc,
            &trust_store,
            untrusted_verdict,
        )
        .iter()
        .any(|p| p.manifest.name == "cfgpath-probe");
    assert!(
            !untrusted_found,
            "untrusted folder must EXCLUDE the ConfigPath plugin from discovery"
        );
    crate::agent::folder_trust::grant_folder_trust(cwd);
    crate::agent::folder_trust::resolve_and_record(cwd, None, false);
    let trusted_dc = resolve_effective_plugins_config(cwd).to_discovery_config();
    let trusted_verdict = crate::agent::folder_trust::project_scope_allowed(cwd);
    assert!(trusted_verdict, "a store-granted repo must resolve trusted");
    let trusted_found = discover_plugins(
            Some(cwd),
            &trusted_dc,
            &trust_store,
            trusted_verdict,
        )
        .iter()
        .any(|p| p.manifest.name == "cfgpath-probe");
    assert!(
            trusted_found,
            "trusted folder must DISCOVER the merged ConfigPath plugin"
        );
}
/// Kill-switch ordering regression: `resolve_effective_plugins_config` reads
/// the folder-trust gate internally, so its call sites (commands/list, plugin
/// fan-out, reload) resolve with the REAL RemoteSettings first. A cold key
/// under an org kill-switch must end up allowed — if the plugins-config read
/// ran first, the gate's remote-less backstop would record a durable
/// kill-switch-blind deny that `resolve_and_record_inner`'s `Some(false)`
/// arm (store-only reconcile) could never lift. GROW_HOME-isolated (empty
/// store); GROW_FOLDER_TRUST unset so the kill-switch is the only signal.
#[test]
#[serial_test::serial]
fn kill_switched_cold_cwd_stays_allowed_through_plugins_config_read() {
    use test_support::EnvGuard;
    let home = tempfile::tempdir().unwrap();
    let _env = EnvGuard::set("GROW_HOME", home.path());
    let _flag = EnvGuard::unset("GROW_FOLDER_TRUST");
    let _sim = simulate_release_build();
    let repo = tempfile::tempdir().unwrap();
    git2::Repository::init(repo.path()).unwrap();
    let grow = repo.path().join(".grow");
    std::fs::create_dir_all(&grow).unwrap();
    std::fs::write(grow.join("config.toml"), "[plugins]\npaths = [\"./proj-plugin\"]\n")
        .unwrap();
    let cwd = repo.path();
    let remote = crate::util::config::RemoteSettings {
        folder_trust_enabled: Some(false),
        ..Default::default()
    };
    assert!(
            crate::agent::folder_trust::resolve_and_record(cwd, Some(&remote), false),
            "kill-switch must resolve the cold key trusted"
        );
    let cfg = resolve_effective_plugins_config(cwd);
    assert!(
            cfg.paths.contains(&"./proj-plugin".to_string()),
            "kill-switched folder counts trusted, so the project path must merge"
        );
    assert!(
            crate::agent::folder_trust::project_scope_allowed(cwd),
            "gate must still allow the kill-switched folder after the config read"
        );
}
