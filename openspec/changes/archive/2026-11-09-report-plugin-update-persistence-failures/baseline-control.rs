#![allow(dead_code)]
pub use shell::config;
mod plugin;
use agent::plugins::git_install::{self, InstallSource};
use agent::plugins::install_registry::InstallRegistry;
use std::path::Path;
fn git(cwd: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}
fn manifest(path: &Path, name: &str) {
    std::fs::create_dir_all(path).unwrap();
    std::fs::write(
        path.join("plugin.json"),
        format!(r#"{{"name":"{name}","version":"1.0.0"}}"#),
    )
    .unwrap();
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let baseline = args[1] == "old";
    let phase = &args[2];
    let home = std::env::current_dir().unwrap();
    let source = home.join("source");
    manifest(&source.join("original"), "existing");
    let install_source = if phase == "update-save" {
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
        git(&source, &["init", "--initial-branch=main"]);
        git(&source, &["config", "user.name", "Baseline fixture"]);
        git(
            &source,
            &["config", "user.email", "baseline@example.invalid"],
        );
        git(&source, &["add", "."]);
        git(&source, &["commit", "-m", "initial"]);
        git(
            &source,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&source, &["push", "-u", "origin", "main"]);
        InstallSource::Git {
            url: remote.to_string_lossy().into_owned(),
            git_ref: Some("main".into()),
            git_sha: None,
            subdir: None,
        }
    } else {
        InstallSource::Local {
            path: source.clone(),
            subdir: None,
        }
    };
    let install_dir = home.join(".grow/installed-plugins");
    let mut registry = InstallRegistry::empty(install_dir.clone());
    let installed = git_install::install_from_source(&install_source, &registry, false).unwrap();
    registry.insert(
        installed.repo_key.clone(),
        git_install::build_installed_repo(&installed, &install_source),
    );
    registry.save().unwrap();
    let registry_path = install_dir.join("registry.json");
    let before = std::fs::read(&registry_path).unwrap();
    let expected_head = if phase == "update-save" {
        manifest(&source.join("added"), "added");
        git(&source, &["add", "."]);
        git(&source, &["commit", "-m", "advance"]);
        git(&source, &["push", "origin", "main"]);
        Some(git(&source, &["rev-parse", "HEAD"]))
    } else {
        None
    };
    if phase == "uninstall-remove" {
        std::fs::remove_dir_all(&installed.repo_path).unwrap();
        std::fs::write(&installed.repo_path, "unexpected file").unwrap();
    } else {
        unsafe { std::env::set_var("GROW_TEST_FAIL_REGISTRY_SAVE_AFTER_SERIALIZE", "1") };
    }
    let result = match (baseline, phase.as_str()) {
        (true, "update-save") => plugin::update_plugins(Some("existing"))
            .map(|_| ())
            .map_err(|e| e.to_string()),
        (false, "update-save") => shell::plugin::update_plugins(Some("existing"))
            .map(|_| ())
            .map_err(|e| e.to_string()),
        (true, _) => plugin::uninstall_plugin("existing", false, true)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        (false, _) => shell::plugin::uninstall_plugin("existing", false, true)
            .map(|_| ())
            .map_err(|e| e.to_string()),
    };
    unsafe { std::env::remove_var("GROW_TEST_FAIL_REGISTRY_SAVE_AFTER_SERIALIZE") };
    let unchanged = std::fs::read(&registry_path).unwrap() == before;
    println!(
        "mode={} phase={phase} result={result:?} artifact_exists={} registry_bytes_unchanged={unchanged}",
        args[1],
        installed.repo_path.exists()
    );
    if let Some(expected_head) = expected_head {
        let head = git(&installed.repo_path, &["rev-parse", "HEAD"]);
        println!("checkout={head} expected={expected_head}");
        assert_eq!(head, expected_head);
    }
    assert!(
        result.is_err(),
        "operation falsely reported success after injected failure"
    );
    assert!(
        unchanged,
        "failed operation must preserve the previous registry bytes"
    );
    match phase.as_str() {
        "uninstall-save" => assert!(!installed.repo_path.exists()),
        "uninstall-remove" => assert!(installed.repo_path.is_file()),
        _ => {}
    }
}
