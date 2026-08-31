use super::*;

fn config(models: &str, default: &str) -> config::Config {
    let raw: toml::Value = toml::from_str(&format!(
        r#"
        [models]
        default = "{default}"

        [provider.local]
        api_backend = "chat_completions"

        [provider.local.options]
        base_url = "https://llm.example/v1"
        api_key = "test-key"

        {models}
        "#
    ))
    .unwrap();
    config::Config::new_from_toml_cfg(&raw).unwrap()
}

fn two_model_config(default: &str) -> config::Config {
    config(
        r#"
        [provider.local.models.alpha]
        name = "Alpha"
        context_window = 100000
        reasoning_efforts = ["low", "high"]

        [provider.local.models.beta]
        name = "Beta"
        context_window = 200000
        "#,
        default,
    )
}

#[test]
fn from_config_uses_only_explicit_provider_models() {
    let manager = ModelsManager::from_config(&two_model_config("local/beta")).unwrap();
    assert_eq!(manager.current_model_id().0.as_ref(), "local/beta");
    assert_eq!(
        manager
            .models()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["local/alpha", "local/beta"]
    );
    assert_eq!(manager.available().len(), 2);
}

#[test]
fn apply_config_preserves_an_existing_session_selection() {
    let manager = ModelsManager::from_config(&two_model_config("local/alpha")).unwrap();
    let revision = manager.catalog_revision();
    manager.set_current_model_id(crate::agent::models::ModelId::new("local/beta"));
    manager
        .apply_config(two_model_config("local/alpha"))
        .unwrap();
    assert_eq!(manager.current_model_id().0.as_ref(), "local/beta");
    assert_eq!(manager.catalog_revision(), revision + 1);
}

#[test]
fn apply_config_reselects_when_current_model_disappears() {
    let manager = ModelsManager::from_config(&two_model_config("local/alpha")).unwrap();
    manager.set_current_model_id(crate::agent::models::ModelId::new("local/beta"));
    let next = config(
        r#"
        [provider.local.models.alpha]
        name = "Alpha"
        "#,
        "local/alpha",
    );
    manager.apply_config(next).unwrap();
    assert_eq!(manager.current_model_id().0.as_ref(), "local/alpha");
}

#[test]
fn explicit_selection_bumps_model_switch_generation_once() {
    let manager = ModelsManager::from_config(&two_model_config("local/alpha")).unwrap();
    let before = manager.model_switch_generation();
    manager.set_current_model_id(crate::agent::models::ModelId::new("local/beta"));
    assert_eq!(manager.model_switch_generation(), before + 1);
    manager.set_current_model_id(crate::agent::models::ModelId::new("local/beta"));
    assert_eq!(manager.model_switch_generation(), before + 1);
}

#[test]
fn configured_filters_fail_closed() {
    let mut cfg = two_model_config("local/alpha");
    cfg.models.allowed_models = Some(vec!["local/missing".into()]);
    let error = match ModelsManager::from_config(&cfg) {
        Ok(_) => panic!("an empty allowlist must be rejected"),
        Err(error) => error,
    };
    assert!(error.contains("allowed_models"), "{error}");
}

#[test]
fn task_model_error_lists_provider_qualified_ids() {
    let manager = ModelsManager::from_config(&two_model_config("local/alpha")).unwrap();
    assert!(manager.task_model_error("local/alpha").is_none());
    let error = manager.task_model_error("missing").unwrap();
    assert!(error.contains("local/alpha"), "{error}");
    assert!(error.contains("local/beta"), "{error}");
}

#[test]
fn task_selectable_projection_matches_task_model_validation() {
    let mut cfg = two_model_config("local/alpha");
    cfg.models.allowed_models = Some(vec!["local/alpha".into()]);
    let manager = ModelsManager::from_config(&cfg).unwrap();
    let selectable = manager.task_selectable_models();
    assert_eq!(
        selectable.keys().map(String::as_str).collect::<Vec<_>>(),
        ["local/alpha"]
    );
    for model_id in manager.models().keys() {
        assert_eq!(
            selectable.contains_key(model_id),
            manager.task_model_error(model_id).is_none(),
            "projection and Task validator diverged for {model_id}"
        );
    }
}

#[test]
fn sampling_config_uses_selected_provider_credentials() {
    let manager = ModelsManager::from_config(&two_model_config("local/alpha")).unwrap();
    let sampling = manager.sampling_config();
    assert_eq!(sampling.base_url, "https://llm.example/v1");
    assert_eq!(sampling.api_key.as_deref(), Some("test-key"));
}

#[test]
fn invalid_reload_leaves_the_live_snapshot_unchanged() {
    let manager = ModelsManager::from_config(&two_model_config("local/alpha")).unwrap();
    manager.set_current_model_id(crate::agent::models::ModelId::new("local/beta"));
    let before_models = manager.models();
    let before_current = manager.current_model_id();
    let before_route = manager
        .sampling_config_for_model("local/beta")
        .unwrap()
        .base_url;

    let mut invalid = two_model_config("local/alpha");
    invalid.models.allowed_models = Some(vec!["local/missing".into()]);
    assert!(manager.apply_config(invalid).is_err());

    assert_eq!(manager.models().len(), before_models.len());
    assert_eq!(manager.current_model_id(), before_current);
    assert_eq!(
        manager
            .sampling_config_for_model("local/beta")
            .unwrap()
            .base_url,
        before_route
    );
}

#[test]
fn live_sampling_lookup_uses_reloaded_provider_route() {
    let manager = ModelsManager::from_config(&two_model_config("local/alpha")).unwrap();
    assert_eq!(
        manager
            .sampling_config_for_model("local/alpha")
            .unwrap()
            .base_url,
        "https://llm.example/v1"
    );
    let mut next = two_model_config("local/alpha");
    next.config_models.values_mut().for_each(|model| {
        model.base_url = Some("https://new.example/v2".into());
    });
    manager.apply_config(next).unwrap();
    assert_eq!(
        manager
            .sampling_config_for_model("local/alpha")
            .unwrap()
            .base_url,
        "https://new.example/v2"
    );
}

#[test]
fn reasoning_efforts_come_from_the_selected_catalog_entry() {
    let manager = ModelsManager::from_config(&two_model_config("local/alpha")).unwrap();
    let efforts = manager.model_reasoning_efforts("local/alpha");
    assert_eq!(efforts.len(), 2);
    assert!(manager.model_reasoning_efforts("local/beta").is_empty());
}
