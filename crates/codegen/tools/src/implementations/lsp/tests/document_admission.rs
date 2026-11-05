use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use super::*;
use crate::implementations::lsp::capabilities::SavePolicy;
use crate::implementations::lsp::documents::FIRST_VERSION;

// A watchdog for broken synchronization, never the mechanism ordering events.
const WATCHDOG: Duration = Duration::from_secs(5);

fn receive<T>(receiver: &Receiver<T>) -> T {
    receiver
        .recv_timeout(WATCHDOG)
        .expect("admission test stalled")
}

fn queued_client() -> (LspClient, LspMainLoop) {
    let diagnostics = DiagnosticsStore::new();
    let documents = Documents::new();
    let notify = Arc::new(tokio::sync::Notify::new());
    let refresh = RefreshTarget::new();
    let (main_loop, socket) = create_client_main_loop(
        "admission",
        diagnostics.clone(),
        documents.clone(),
        notify.clone(),
        refresh.clone(),
    );
    let pull = PullDiagnostics::new(
        "admission",
        socket.clone(),
        diagnostics.clone(),
        documents.clone(),
        notify,
    );
    let client = LspClient {
        server_name: "admission".into(),
        lifecycle_id: 1,
        socket,
        diagnostics,
        documents,
        policy: ServerPolicy {
            sync_incremental: true,
            save: SavePolicy::Skip,
            advertises_pull: false,
        },
        pull,
        refresh,
        main_loop: tokio::spawn(std::future::pending()),
        stderr_task: None,
        child_process: None,
        process_group: None,
        shutdown_timeout: WATCHDOG,
        document_update_queued: None,
    };
    // Keeping the real MainLoop alive makes ServerSocket's synchronous queue
    // accept notifications without introducing transport scheduling into this test.
    (client, main_loop)
}

fn push_during_admission(change: bool, versioned: bool) {
    let (mut client, _main_loop) = queued_client();
    let workspace = tempfile::tempdir().unwrap();
    let file = workspace.path().join("admission.ts");
    let uri = file_uri(&file).unwrap().to_string();
    // Suppress independent pulls; the push below must be the only verdict.
    client.diagnostics.record_push(&uri, vec![], None, None);
    if change {
        assert_eq!(
            client.notify_file_change(&file, "old\n🚀", "typescript"),
            Some(FIRST_VERSION)
        );
    }
    let expected = FIRST_VERSION + i32::from(change);
    let documents = client.documents.clone();
    let store = client.diagnostics.clone();
    let (queued_tx, queued_rx) = mpsc::sync_channel(0);
    let (release_tx, release_rx) = mpsc::sync_channel(0);
    client.document_update_queued = Some(Box::new(move || {
        queued_tx.send(()).unwrap();
        receive(&release_rx);
    }));

    let writer = std::thread::spawn(move || {
        let sent = client.notify_file_change(&file, "new text\n", "typescript");
        (client, sent)
    });
    receive(&queued_rx);

    let (attempted_tx, attempted_rx) = mpsc::sync_channel(0);
    let (recorded_tx, recorded_rx) = mpsc::sync_channel(0);
    let reader_uri = uri.clone();
    let reader_store = store.clone();
    let reader = std::thread::spawn(move || {
        let blocked = documents.read_is_blocked();
        attempted_tx.send(blocked).unwrap();
        // This is the production router's version read and real store write.
        let latest_sent = documents.version(&reader_uri);
        reader_store.record_push(
            &reader_uri,
            vec![Diagnostic {
                message: "fast push".into(),
                ..Default::default()
            }],
            versioned.then_some(expected),
            latest_sent,
        );
        recorded_tx.send(()).unwrap();
    });
    let blocked = receive(&attempted_rx);
    // On the old path a reader can finish before commit. Force it to do so,
    // making the regression deterministic. With atomic admission it must wait
    // for the writer's release instead. No scheduling delay selects the path.
    if !blocked {
        receive(&recorded_rx);
    }
    release_tx.send(()).unwrap();
    if blocked {
        receive(&recorded_rx);
    }
    reader.join().unwrap();
    let (client, sent) = writer.join().unwrap();
    assert_eq!(sent, Some(expected));
    assert_eq!(client.documents.version(&uri), Some(expected));
    assert_eq!(store.items(&uri)[0].message, "fast push");
    assert_eq!(
        store.covers(&uri),
        Some(expected),
        "a push after successful notification admission must cover that text"
    );
    assert!(store.answered_for(&uri, expected));
    assert!(
        blocked,
        "the diagnostic reader must wait for admission to commit"
    );
}

#[tokio::test]
async fn did_open_versioned_push_waits_for_admission() {
    push_during_admission(false, true);
}

#[tokio::test]
async fn did_open_versionless_push_waits_for_admission() {
    push_during_admission(false, false);
}

#[tokio::test]
async fn did_change_versioned_push_waits_for_admission() {
    push_during_admission(true, true);
}

#[tokio::test]
async fn did_change_versionless_push_waits_for_admission() {
    push_during_admission(true, false);
}
