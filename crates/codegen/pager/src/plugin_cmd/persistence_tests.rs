//! Exercise the public CLI dispatcher in a fresh process for each persistence outcome.

use super::{PluginArgs, PluginCommand, run};
use agent::plugins::git_install::{self, InstallSource};
use agent::plugins::install_registry::{InstallKind, InstallRegistry};
use std::path::Path;

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn manifest(path: &Path, name: &str, version: &str) {
    std::fs::create_dir_all(path).unwrap();
    std::fs::write(
        path.join("plugin.json"),
        serde_json::json!({"name": name, "version": version}).to_string(),
    )
    .unwrap();
}

#[test]
fn public_uninstall_and_update_dispatch_propagate_persistence_failures() {
    const TEST: &str = "plugin_cmd::persistence_tests::public_uninstall_and_update_dispatch_propagate_persistence_failures";
    const PHASE: &str = "GROW_TEST_CLI_PLUGIN_PERSISTENCE_PHASE";
    let Ok(phase) = std::env::var(PHASE) else {
        for action in ["uninstall", "update"] {
            for failure in ["save", "parse", "read", "success", "noop"] {
                let phase = format!("{action}-{failure}");
                let temp = tempfile::tempdir().unwrap();
                let home = dunce::canonicalize(temp.path()).unwrap();
                std::fs::create_dir_all(home.join(".grow")).unwrap();
                let output = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([TEST, "--exact", "--nocapture", "--test-threads=1"])
                    .env(PHASE, &phase)
                    .env("HOME", &home)
                    .env("GROW_HOME", home.join(".grow"))
                    .env("GROW_MARKETPLACE_REQUIRE_SHA", "0")
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .env("GIT_CONFIG_GLOBAL", home.join("absent-gitconfig"))
                    .env_remove("GROW_TEST_FAIL_REGISTRY_SAVE_AFTER_SERIALIZE")
                    .current_dir(&home)
                    .output()
                    .unwrap();
                let stdout = String::from_utf8_lossy(&output.stdout);
                assert!(
                    output.status.success(),
                    "{phase}:\n{stdout}\n{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(stdout.contains("1 passed"), "{phase}: {stdout}");
                let reported_success =
                    stdout.contains("Uninstalled 1 plugin(s)") || stdout.contains(": updated (");
                assert_eq!(reported_success, failure == "success", "{phase}: {stdout}");
                if action == "update" && failure == "noop" {
                    assert!(stdout.contains("already up to date"), "{stdout}");
                }
            }
        }
        return;
    };
    let (action, failure) = phase.split_once('-').unwrap();
    let home = std::env::current_dir().unwrap();
    let install_dir = home.join(".grow/installed-plugins");
    let work = home.join("source");
    manifest(&work.join("original"), "cli-plugin", "1.0.0");
    let mut expected_commit = None;
    let source = if action == "update" {
        let remote = home.join("remote.git");
        git(
            &home,
            &[
                "init",
                "--bare",
                "--initial-branch=main",
                remote.to_str().unwrap(),
            ],
        );
        git(&work, &["init", "--initial-branch=main"]);
        git(&work, &["config", "user.name", "CLI fixture"]);
        git(
            &work,
            &["config", "user.email", "cli-fixture@example.invalid"],
        );
        git(&work, &["add", "."]);
        git(&work, &["commit", "-m", "original"]);
        git(
            &work,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&work, &["push", "-u", "origin", "main"]);
        InstallSource::Git {
            url: remote.to_string_lossy().into_owned(),
            git_ref: Some("main".into()),
            git_sha: None,
            subdir: None,
        }
    } else {
        InstallSource::Local {
            path: work.clone(),
            subdir: None,
        }
    };
    let mut registry = InstallRegistry::empty(install_dir.clone());
    let installed = git_install::install_from_source(&source, &registry, false).unwrap();
    registry.insert(
        installed.repo_key.clone(),
        git_install::build_installed_repo(&installed, &source),
    );
    registry.save().unwrap();
    let old_commit = if action == "update" {
        Some(git(&installed.repo_path, &["rev-parse", "HEAD"]))
    } else {
        None
    };
    if action == "update" && failure != "noop" {
        manifest(&work.join("original"), "cli-plugin", "2.0.0");
        manifest(&work.join("added"), "cli-added", "1.0.0");
        git(&work, &["add", "."]);
        git(&work, &["commit", "-m", "advance"]);
        git(&work, &["push", "origin", "main"]);
        expected_commit = Some(git(&work, &["rev-parse", "HEAD"]));
    }
    let registry_path = install_dir.join("registry.json");
    match failure {
        "parse" => std::fs::write(&registry_path, "{ broken registry").unwrap(),
        "read" => {
            std::fs::remove_file(&registry_path).unwrap();
            std::fs::create_dir(&registry_path).unwrap();
        }
        "save" => unsafe { std::env::set_var("GROW_TEST_FAIL_REGISTRY_SAVE_AFTER_SERIALIZE", "1") },
        "noop" if action == "uninstall" => {
            std::fs::remove_dir_all(&installed.repo_path).unwrap();
            std::fs::write(&installed.repo_path, "unexpected file").unwrap();
        }
        "noop" => unsafe { std::env::set_var("GROW_TEST_FAIL_REGISTRY_SAVE_AFTER_SERIALIZE", "1") },
        "success" => {}
        other => panic!("unknown phase {other}"),
    }
    let before = std::fs::read(&registry_path).ok();
    let command = if action == "uninstall" {
        PluginCommand::Uninstall {
            name: "cli-plugin".into(),
            confirm: false,
            keep_data: true,
        }
    } else {
        PluginCommand::Update {
            name: Some("cli-plugin".into()),
        }
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = rt.block_on(run(PluginArgs { command }));
    unsafe { std::env::remove_var("GROW_TEST_FAIL_REGISTRY_SAVE_AFTER_SERIALIZE") };
    if failure == "success" {
        result.unwrap();
        let durable = InstallRegistry::try_load_from(install_dir).unwrap();
        if action == "uninstall" {
            assert!(!installed.repo_path.exists());
            assert!(durable.get_repo(&installed.repo_key).is_none());
        } else {
            let repo = durable.get_repo(&installed.repo_key).unwrap();
            match &repo.kind {
                InstallKind::Git { commit, .. } => {
                    assert_eq!(Some(commit), expected_commit.as_ref())
                }
                _ => panic!("expected git registration"),
            }
            assert!(repo.plugins.contains_key("cli-added"));
            assert_eq!(repo.plugins["cli-plugin"].version.as_deref(), Some("2.0.0"));
        }
        return;
    }
    if failure == "read" {
        assert!(registry_path.is_dir());
    } else {
        assert_eq!(std::fs::read(&registry_path).unwrap(), before.unwrap());
    }
    if action == "update" {
        let expected = if failure == "save" {
            expected_commit.as_ref()
        } else {
            old_commit.as_ref()
        };
        assert_eq!(
            Some(&git(&installed.repo_path, &["rev-parse", "HEAD"])),
            expected
        );
    } else {
        assert_eq!(installed.repo_path.exists(), failure != "save");
    }
    if failure == "noop" && action == "update" {
        result.unwrap();
    } else {
        let error = result
            .expect_err("public dispatch must propagate incomplete operation")
            .to_string();
        if failure == "save" {
            assert!(
                error.contains("incomplete")
                    && error.contains("test-injected registry save failure"),
                "{error}"
            );
            assert!(
                error.contains(&installed.repo_key)
                    && error.contains(installed.repo_path.to_str().unwrap()),
                "{error}"
            );
        } else if failure == "noop" {
            assert!(error.contains("still exists"), "{error}");
        } else {
            assert!(error.contains("failed to load registry"), "{error}");
        }
    }
}
