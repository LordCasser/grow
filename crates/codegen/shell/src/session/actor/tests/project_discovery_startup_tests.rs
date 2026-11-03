use super::*;

#[tokio::test]
async fn command_ack_progresses_while_project_watcher_registration_is_blocked() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let local_thread = std::thread::current().id();
            let shutdown = tokio_util::sync::CancellationToken::new();
            let (registration_started_tx, registration_started_rx) =
                tokio::sync::oneshot::channel();
            let (registration_release_tx, registration_release_rx) = std::sync::mpsc::channel();
            let (owner_dropped_tx, owner_dropped_rx) = tokio::sync::oneshot::channel();
            let (_, native_change_rx) =
                tokio::sync::mpsc::unbounded_channel::<crate::config::watcher::DiscoveryChange>();

            struct OwnerDrop {
                dropped: Option<tokio::sync::oneshot::Sender<std::thread::ThreadId>>,
            }
            impl Drop for OwnerDrop {
                fn drop(&mut self) {
                    if let Some(dropped) = self.dropped.take() {
                        let _ = dropped.send(std::thread::current().id());
                    }
                }
            }

            let worker = super::super::run_loop::spawn_blocking_project_discovery_watcher(
                tokio::runtime::Handle::current(),
                move || {
                    let _ = registration_started_tx.send(std::thread::current().id());
                    registration_release_rx
                        .recv()
                        .expect("test releases the blocked native registration");
                    Some((
                        OwnerDrop {
                            dropped: Some(owner_dropped_tx),
                        },
                        native_change_rx,
                    ))
                },
                shutdown.clone(),
                |_| {},
                |_| async {},
            );

            // The actor command loop remains available while native startup is
            // blocked on its dedicated blocking-runtime thread.
            let blocking_thread = registration_started_rx.await.unwrap();
            assert_ne!(blocking_thread, local_thread);
            let (command_tx, mut command_rx) = tokio::sync::mpsc::unbounded_channel();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            tokio::task::spawn_local(async move {
                let command = command_rx.recv().await;
                let _ = ack_tx.send(command);
            });
            command_tx.send("install-coordination-backend").unwrap();
            assert_eq!(ack_rx.await.unwrap(), Some("install-coordination-backend"));
            assert!(!worker.is_finished());

            worker.abort();
            assert!(worker.await.is_err());
            registration_release_tx.send(()).unwrap();
            assert_eq!(
                owner_dropped_rx
                    .await
                    .expect("an aborted observer leaves the blocking owner to drop its result"),
                blocking_thread
            );
        })
        .await;
}

#[tokio::test]
async fn project_watcher_refresh_and_drop_stay_on_blocking_worker() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let local_thread = std::thread::current().id();
            let shutdown = tokio_util::sync::CancellationToken::new();
            let (native_change_tx, native_change_rx) = tokio::sync::mpsc::unbounded_channel();
            let (refresh_tx, refresh_rx) = tokio::sync::oneshot::channel();
            let (owner_dropped_tx, owner_dropped_rx) = tokio::sync::oneshot::channel();
            let (event_seen_tx, event_seen_rx) = tokio::sync::oneshot::channel();
            let mut event_seen_tx = Some(event_seen_tx);

            struct OwnerDrop {
                refreshed: Option<tokio::sync::oneshot::Sender<std::thread::ThreadId>>,
                dropped: Option<tokio::sync::oneshot::Sender<std::thread::ThreadId>>,
            }
            impl Drop for OwnerDrop {
                fn drop(&mut self) {
                    if let Some(dropped) = self.dropped.take() {
                        let _ = dropped.send(std::thread::current().id());
                    }
                }
            }

            let worker = super::super::run_loop::spawn_blocking_project_discovery_watcher(
                tokio::runtime::Handle::current(),
                move || {
                    Some((
                        OwnerDrop {
                            refreshed: Some(refresh_tx),
                            dropped: Some(owner_dropped_tx),
                        },
                        native_change_rx,
                    ))
                },
                shutdown.clone(),
                |owner| {
                    if let Some(refreshed) = owner.refreshed.take() {
                        let _ = refreshed.send(std::thread::current().id());
                    }
                },
                move |change| {
                    assert_eq!(change, crate::config::watcher::DiscoveryChange::Skills);
                    if let Some(event_seen_tx) = event_seen_tx.take() {
                        let _ = event_seen_tx.send(());
                    }
                    async {}
                },
            );

            native_change_tx
                .send(crate::config::watcher::DiscoveryChange::Skills)
                .unwrap();
            let refresh_thread = refresh_rx.await.unwrap();
            assert_ne!(refresh_thread, local_thread);
            event_seen_rx.await.unwrap();
            shutdown.cancel();
            worker.await.unwrap();
            assert_eq!(owner_dropped_rx.await.unwrap(), refresh_thread);
        })
        .await;
}

#[tokio::test]
async fn session_runtime_and_writer_ack_finish_before_host_watcher_owner() {
    struct OwnerDrop(Option<tokio::sync::oneshot::Sender<std::thread::ThreadId>>);
    impl Drop for OwnerDrop {
        fn drop(&mut self) {
            if let Some(dropped) = self.0.take() {
                let _ = dropped.send(std::thread::current().id());
            }
        }
    }

    struct ReleaseOnDrop(Option<std::sync::mpsc::Sender<()>>);
    impl Drop for ReleaseOnDrop {
        fn drop(&mut self) {
            if let Some(release) = self.0.take() {
                let _ = release.send(());
            }
        }
    }

    let host_runtime = tokio::runtime::Handle::current();
    let (native_started_host_tx, native_started_host_rx) = tokio::sync::oneshot::channel();
    let (native_started_session_tx, native_started_session_rx) = tokio::sync::oneshot::channel();
    let (native_release_tx, native_release_rx) = std::sync::mpsc::channel();
    let mut release_on_drop = ReleaseOnDrop(Some(native_release_tx));
    let (owner_dropped_tx, mut owner_dropped_rx) = tokio::sync::oneshot::channel();
    let (writer_acked_tx, writer_acked_rx) = tokio::sync::oneshot::channel();
    let (session_runtime_dropped_tx, session_runtime_dropped_rx) = tokio::sync::oneshot::channel();

    let session_thread = std::thread::spawn(move || {
        let session_thread_id = std::thread::current().id();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        local.block_on(&runtime, async move {
            let shutdown = tokio_util::sync::CancellationToken::new();
            let (_native_changes_tx, native_changes_rx) = tokio::sync::mpsc::unbounded_channel();
            let watcher = super::super::run_loop::spawn_blocking_project_discovery_watcher(
                host_runtime,
                move || {
                    let thread = std::thread::current().id();
                    let _ = native_started_host_tx.send(thread);
                    let _ = native_started_session_tx.send(thread);
                    native_release_rx
                        .recv()
                        .expect("test releases blocked native start");
                    Some((OwnerDrop(Some(owner_dropped_tx)), native_changes_rx))
                },
                shutdown.clone(),
                |_| {},
                |_| async {},
            );

            let _native_thread = native_started_session_rx.await.unwrap();
            let (writer_tx, mut writer_rx) = tokio::sync::mpsc::unbounded_channel();
            let persistence =
                crate::session::persistence::PersistenceHandle::from_sender_for_test(writer_tx);
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            persistence
                .tx
                .send(crate::session::persistence::PersistenceMsg::FlushAndAck {
                    respond_to: ack_tx,
                })
                .unwrap();
            match writer_rx.recv().await.unwrap() {
                crate::session::persistence::PersistenceMsg::FlushAndAck { respond_to } => {
                    let _ = respond_to.send(());
                }
                _ => panic!("writer received an unexpected persistence message"),
            }
            ack_rx.await.unwrap();
            let _ = writer_acked_tx.send(());

            shutdown.cancel();
            watcher.await.unwrap();
        });
        drop(local);
        drop(runtime);
        let _ = session_runtime_dropped_tx.send(session_thread_id);
    });

    let native_thread = native_started_host_rx.await.unwrap();
    writer_acked_rx.await.unwrap();
    let session_thread_id = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        session_runtime_dropped_rx,
    )
    .await
    .expect("session runtime shutdown must not wait for native start")
    .unwrap();
    assert!(matches!(
        owner_dropped_rx.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));

    release_on_drop.0.take().unwrap().send(()).unwrap();
    let owner_thread = tokio::time::timeout(std::time::Duration::from_secs(5), owner_dropped_rx)
        .await
        .expect("host blocking owner drops watcher after native start returns")
        .unwrap();
    let joined = tokio::task::spawn_blocking(move || session_thread.join())
        .await
        .unwrap();
    assert!(joined.is_ok());
    assert_ne!(owner_thread, session_thread_id);
    assert_eq!(owner_thread, native_thread);
}

#[tokio::test]
async fn host_runtime_shutdown_cancels_idle_project_watcher_without_session_cancel() {
    struct OwnerDrop(Option<tokio::sync::oneshot::Sender<std::thread::ThreadId>>);
    impl Drop for OwnerDrop {
        fn drop(&mut self) {
            if let Some(dropped) = self.0.take() {
                let _ = dropped.send(std::thread::current().id());
            }
        }
    }

    let (host_handle_tx, host_handle_rx) = std::sync::mpsc::channel();
    let (host_shutdown_tx, host_shutdown_rx) = tokio::sync::oneshot::channel();
    let (host_dropped_tx, host_dropped_rx) = tokio::sync::oneshot::channel();
    let host_thread = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        host_handle_tx.send(runtime.handle().clone()).unwrap();
        runtime.block_on(host_shutdown_rx).unwrap();
        drop(runtime);
        let _ = host_dropped_tx.send(std::thread::current().id());
    });
    let host_runtime = host_handle_rx.recv().unwrap();

    let (native_started_tx, native_started_rx) = tokio::sync::oneshot::channel();
    let (owner_dropped_tx, owner_dropped_rx) = tokio::sync::oneshot::channel();
    let (session_done_tx, session_done_rx) = tokio::sync::oneshot::channel();
    let (session_thread_id_tx, session_thread_id_rx) = tokio::sync::oneshot::channel();
    let session_thread = std::thread::spawn(move || {
        let session_thread_id = std::thread::current().id();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        local.block_on(&runtime, async move {
            let shutdown = tokio_util::sync::CancellationToken::new();
            let (_native_changes_tx, native_changes_rx) = tokio::sync::mpsc::unbounded_channel();
            let observer = super::super::run_loop::spawn_blocking_project_discovery_watcher(
                host_runtime,
                move || {
                    let _ = native_started_tx.send(std::thread::current().id());
                    Some((OwnerDrop(Some(owner_dropped_tx)), native_changes_rx))
                },
                shutdown,
                |_| {},
                |_| async {},
            );
            observer.await.unwrap();
            let _ = session_thread_id_tx.send(session_thread_id);
        });
        drop(local);
        drop(runtime);
        let _ = session_done_tx.send(());
    });

    let native_thread = native_started_rx.await.unwrap();
    host_shutdown_tx.send(()).unwrap();
    let owner_thread = tokio::time::timeout(std::time::Duration::from_secs(5), owner_dropped_rx)
        .await
        .expect("host guardian cancellation releases the idle native owner")
        .unwrap();
    let host_thread_id = tokio::time::timeout(std::time::Duration::from_secs(5), host_dropped_rx)
        .await
        .expect("host runtime shutdown completes after its watcher worker exits")
        .unwrap();
    let session_thread_id =
        tokio::time::timeout(std::time::Duration::from_secs(5), session_thread_id_rx)
            .await
            .expect("session observer finishes after its host guardian is dropped")
            .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), session_done_rx)
        .await
        .expect("session runtime exits after its observer")
        .unwrap();
    let host_join = tokio::task::spawn_blocking(move || host_thread.join())
        .await
        .unwrap();
    let session_join = tokio::task::spawn_blocking(move || session_thread.join())
        .await
        .unwrap();

    assert!(host_join.is_ok());
    assert!(session_join.is_ok());
    assert_eq!(owner_thread, native_thread);
    assert_ne!(owner_thread, host_thread_id);
    assert_ne!(owner_thread, session_thread_id);
}
