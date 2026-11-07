use super::*;
use std::path::{Path, PathBuf};
use tokio::io::AsyncBufReadExt;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

fn root(parent: &Path, name: &str) -> String {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    // Match FSEvents' canonical event paths, including macOS /var aliases.
    dunce::canonicalize(&path)
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn register(paths: Vec<String>) -> Option<SkillsWatchRegistration> {
    let dirs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    crate::config::watcher::SkillsFileWatcher::start_with_dirs(
        &dirs,
        Path::new("/unused-grow-home"),
        None,
    )
}

async fn reload(reader: &mut BufReader<tokio::io::DuplexStream>, root: &str) {
    std::fs::write(Path::new(root).join("SKILL.md"), "changed").unwrap();
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(12), reader.read_line(&mut line))
        .await
        .expect("native discovery event must reach ACP")
        .unwrap();
    let request: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(request["method"], "_grow/internal/reload_skills");
}

#[tokio::test(flavor = "current_thread")]
async fn latest_paths_survive_initial_registration_and_removed_root_stops_events() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let temp = tempfile::tempdir().unwrap();
            let a = root(temp.path(), "a");
            let b = root(temp.path(), "b");
            let c = root(temp.path(), "c");
            let (paths_tx, paths_rx) = watch::channel(vec![a.clone()]);
            let (started_tx, mut started_rx) = mpsc::unbounded_channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let release_rx = Arc::new(std::sync::Mutex::new(Some(release_rx)));
            let (writer, reader) = tokio::io::duplex(4096);
            let mut reader = BufReader::new(reader);
            let cancel = CancellationToken::new();
            let task = tokio::task::spawn_local(run_leader_skills_file_watcher(
                paths_rx,
                Arc::new(TokioMutex::new(writer)),
                cancel.clone(),
                move |paths| {
                    let first = release_rx.lock().unwrap().take();
                    if let Some(release) = first {
                        started_tx.send(paths.clone()).unwrap();
                        release.recv().unwrap();
                        register(paths)
                    } else {
                        let result = register(paths.clone());
                        started_tx.send(paths).unwrap();
                        result
                    }
                },
            ));
            assert_eq!(started_rx.recv().await.unwrap(), vec![a.clone()]);
            paths_tx.send_replace(vec![b]);
            paths_tx.send_replace(vec![c.clone()]);
            release_tx.send(()).unwrap();
            assert_eq!(started_rx.recv().await.unwrap(), vec![c.clone()]);
            // An event from C is also an adoption barrier: the latest registration
            // has completed and the previous native owner has been retired.
            reload(&mut reader, &c).await;
            std::fs::write(Path::new(&a).join("removed.md"), "removed").unwrap();
            let mut line = String::new();
            assert!(
                tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
                    .await
                    .is_err(),
                "the removed root still supplied a native event: {line}"
            );
            cancel.cancel();
            task.await.unwrap().unwrap();
            assert_eq!(reader.read_line(&mut line).await.unwrap(), 0);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn failed_replacement_retains_previous_owner_and_later_update_recovers() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let temp = tempfile::tempdir().unwrap();
            let a = root(temp.path(), "a");
            let c = root(temp.path(), "c");
            let (paths_tx, paths_rx) = watch::channel(vec![a.clone()]);
            let (started_tx, mut started_rx) = mpsc::unbounded_channel();
            let (writer, reader) = tokio::io::duplex(4096);
            let mut reader = BufReader::new(reader);
            let cancel = CancellationToken::new();
            let task = tokio::task::spawn_local(run_leader_skills_file_watcher(
                paths_rx,
                Arc::new(TokioMutex::new(writer)),
                cancel.clone(),
                move |paths| {
                    let result = if paths == ["fail"] {
                        None
                    } else {
                        register(paths.clone())
                    };
                    started_tx.send(paths).unwrap();
                    result
                },
            ));
            started_rx.recv().await.unwrap();
            reload(&mut reader, &a).await;
            paths_tx.send_replace(vec!["fail".into()]);
            assert_eq!(started_rx.recv().await.unwrap(), vec!["fail"]);
            std::fs::write(Path::new(&a).join("another.md"), "retained").unwrap();
            let mut line = String::new();
            tokio::time::timeout(Duration::from_secs(12), reader.read_line(&mut line))
                .await
                .unwrap()
                .unwrap();
            assert!(line.contains("_grow/internal/reload_skills"));
            paths_tx.send_replace(vec![c.clone()]);
            started_rx.recv().await.unwrap();
            reload(&mut reader, &c).await;
            cancel.cancel();
            task.await.unwrap().unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn no_initial_watcher_can_recover_on_a_later_paths_update() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let temp = tempfile::tempdir().unwrap();
            let a = root(temp.path(), "a");
            let (paths_tx, paths_rx) = watch::channel(Vec::new());
            let (started_tx, mut started_rx) = mpsc::unbounded_channel();
            let (writer, reader) = tokio::io::duplex(4096);
            let mut reader = BufReader::new(reader);
            let cancel = CancellationToken::new();
            let task = tokio::task::spawn_local(run_leader_skills_file_watcher(
                paths_rx,
                Arc::new(TokioMutex::new(writer)),
                cancel.clone(),
                move |paths| {
                    let result = register(paths.clone());
                    started_tx.send(paths).unwrap();
                    result
                },
            ));
            assert!(started_rx.recv().await.unwrap().is_empty());
            paths_tx.send_replace(vec![a.clone()]);
            started_rx.recv().await.unwrap();
            reload(&mut reader, &a).await;
            cancel.cancel();
            task.await.unwrap().unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_during_replacement_does_not_wait_for_os_registration() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let temp = tempfile::tempdir().unwrap();
            let a = root(temp.path(), "a");
            let b = root(temp.path(), "b");
            let (paths_tx, paths_rx) = watch::channel(vec![a.clone()]);
            let (started_tx, mut started_rx) = mpsc::unbounded_channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let release_rx = Arc::new(std::sync::Mutex::new(release_rx));
            let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
            let finished_tx = Arc::new(std::sync::Mutex::new(Some(finished_tx)));
            let (writer, reader) = tokio::io::duplex(4096);
            let mut reader = BufReader::new(reader);
            let cancel = CancellationToken::new();
            let task = tokio::task::spawn_local(run_leader_skills_file_watcher(
                paths_rx,
                Arc::new(TokioMutex::new(writer)),
                cancel.clone(),
                move |paths| {
                    let replacement = paths == [b.clone()];
                    if replacement {
                        started_tx.send(paths.clone()).unwrap();
                        release_rx.lock().unwrap().recv().unwrap();
                    }
                    let result = register(paths.clone());
                    if replacement {
                        finished_tx
                            .lock()
                            .unwrap()
                            .take()
                            .unwrap()
                            .send(())
                            .unwrap();
                    } else {
                        started_tx.send(paths).unwrap();
                    }
                    result
                },
            ));
            started_rx.recv().await.unwrap();
            reload(&mut reader, &a).await;
            paths_tx.send_replace(vec![root(temp.path(), "b")]);
            started_rx.recv().await.unwrap();
            cancel.cancel();
            tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .expect("cancellation must not wait for registration")
                .unwrap()
                .unwrap();
            release_tx.send(()).unwrap();
            finished_rx.await.unwrap();
            // finish_blocking_registration's generic owner-drop regression below
            // proves the detached task drops its eventual registration result.
            let mut line = String::new();
            assert_eq!(reader.read_line(&mut line).await.unwrap(), 0);
        })
        .await;
}
