//! Public-handle regressions for reusable request ids and submission ownership.

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::poll;
use sampler::{
    AttemptUsage, AttemptUsageSink, RequestId, RetryPolicy, SamplerActor, SamplerConfig,
    SamplerHandle, SamplingEvent,
};
use sampling_types::{ContentPart, ConversationItem, ConversationRequest, SamplingError, UserItem};
use test_support::{
    InferenceEndpoint, InferenceExpectation, InferenceRequestMatcher, MockInferenceServer,
    ScriptedResponse, SseEvent, sse,
};
use tokio::sync::{mpsc, oneshot};

#[ctor::ctor]
fn install_rustls_provider() {
    diagnostics::tls::install_ring_provider_once();
}

const TIMEOUT: Duration = Duration::from_secs(5);

async fn within<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(TIMEOUT, future)
        .await
        .expect("request lifetime handshake timed out")
}

fn config(server: &MockInferenceServer) -> SamplerConfig {
    SamplerConfig {
        base_url: server.url(),
        model: "test-model".into(),
        max_retries: Some(0),
        ..SamplerConfig::default()
    }
}

fn request(text: &str) -> ConversationRequest {
    ConversationRequest {
        items: vec![ConversationItem::User(UserItem {
            content: vec![ContentPart::Text { text: text.into() }],
            ..Default::default()
        })],
        ..Default::default()
    }
}

fn expect_success(server: &MockInferenceServer, name: &str) -> InferenceExpectation {
    server.expect_response(
        name,
        InferenceRequestMatcher::auxiliary(InferenceEndpoint::ChatCompletions),
        ScriptedResponse::sse(sse::chat_completion_script_exact("finished", "test-model")),
    )
}

fn expect_pending(server: &MockInferenceServer, name: &str) -> InferenceExpectation {
    // Send content without completion evidence, then hold the final frame.
    let first = sse::chat_completion_script_exact("waiting forever", "test-model").remove(0);
    server.expect_response_blocked(
        name,
        InferenceRequestMatcher::auxiliary(InferenceEndpoint::ChatCompletions),
        ScriptedResponse::sse(vec![first, SseEvent::data("[DONE]")]),
    )
}

async fn terminal(rx: &mut mpsc::UnboundedReceiver<SamplingEvent>) -> SamplingEvent {
    within(async {
        loop {
            let event = rx.recv().await.expect("sampler event channel closed");
            if matches!(
                event,
                SamplingEvent::RequestCompleted { .. } | SamplingEvent::Failed { .. }
            ) {
                return event;
            }
        }
    })
    .await
}

fn assert_cancelled(event: SamplingEvent) {
    assert!(matches!(
        event,
        SamplingEvent::Failed { error, .. } if error.message.contains("cancelled")
    ));
}

async fn cancel_current(
    handle: &SamplerHandle,
    id: &RequestId,
    events: &mut mpsc::UnboundedReceiver<SamplingEvent>,
) {
    assert!(within(handle.is_active(id.clone())).await);
    assert_eq!(within(handle.active_count()).await, 1);
    handle.cancel(id.clone());
    assert_cancelled(terminal(events).await);
    assert!(!within(handle.is_active(id.clone())).await);
    assert_eq!(within(handle.active_count()).await, 0);
}

/// One acknowledgment barrier, while retaining every call for exactly-once checks.
fn usage_barrier() -> (
    AttemptUsageSink,
    mpsc::UnboundedReceiver<AttemptUsage>,
    oneshot::Sender<()>,
) {
    let (usage_tx, usage_rx) = mpsc::unbounded_channel();
    let (release_tx, release_rx) = oneshot::channel();
    let release = Mutex::new(Some(release_rx));
    let sink: AttemptUsageSink = Arc::new(move |usage| {
        usage_tx
            .send(usage)
            .expect("usage receiver lives through shutdown");
        let release = release.lock().unwrap().take();
        Box::pin(async move {
            if let Some(release) = release {
                release
                    .await
                    .expect("usage acknowledgment must be released");
            }
            Ok(())
        })
    });
    (sink, usage_rx, release_tx)
}

#[derive(Clone, Copy)]
enum CollectEntry {
    Plain,
    Accounted,
}

async fn collect(
    entry: CollectEntry,
    handle: &SamplerHandle,
    id: RequestId,
) -> Result<(), SamplingError> {
    match entry {
        CollectEntry::Plain => handle.submit_and_collect(id, request("old")).await,
        CollectEntry::Accounted => {
            handle
                .submit_and_collect_accounted(
                    id,
                    request("old"),
                    None,
                    None,
                    None,
                    sampler::RecoveryBudget::default(),
                )
                .await
        }
    }
    .map(|_| ())
}

// The current-thread runtime makes each manual poll enqueue Submit without
// allowing the actor or request task to run until the test awaits a handshake.
#[tokio::test]
async fn replaced_task_late_completion_keeps_current_registration_and_cancel_target() {
    let server = within(MockInferenceServer::start()).await.unwrap();
    let _first = expect_success(&server, "old provider");
    let mut replacement = expect_pending(&server, "replacement provider");
    let (event_tx, mut events) = mpsc::unbounded_channel();
    let mut owner = SamplerActor::spawn_owned(config(&server), RetryPolicy::default(), event_tx);
    let handle = owner.handle();
    let id = RequestId::from("reused");
    let (sink, mut usages, release) = usage_barrier();
    let mut old = Box::pin(handle.submit_and_collect_accounted(
        id.clone(),
        request("old"),
        None,
        Some(sink),
        None,
        sampler::RecoveryBudget::default(),
    ));
    assert!(poll!(old.as_mut()).is_pending());
    assert!(matches!(
        within(usages.recv()).await,
        Some(AttemptUsage::Known { .. })
    ));
    // Remove the collect guard before replacement to isolate JoinSet cleanup.
    drop(old);
    handle.submit_with_config(id.clone(), request("replacement"), config(&server));
    within(replacement.wait_blocked()).await;
    release.send(()).unwrap();
    assert_cancelled(terminal(&mut events).await);
    cancel_current(&handle, &id, &mut events).await;
    within(owner.shutdown()).await.unwrap();
    assert!(
        usages.try_recv().is_err(),
        "old attempt must settle only once"
    );
    assert_eq!(server.request_count(), 2);
}

async fn finished_collect_keeps_replacement(entry: CollectEntry) {
    let server = within(MockInferenceServer::start()).await.unwrap();
    let _first = expect_success(&server, "old provider");
    let mut replacement = expect_pending(&server, "replacement provider");
    let (event_tx, mut events) = mpsc::unbounded_channel();
    let mut owner = SamplerActor::spawn_owned(config(&server), RetryPolicy::default(), event_tx);
    let handle = owner.handle();
    let id = RequestId::from("reused");
    let mut old = Box::pin(collect(entry, &handle, id.clone()));
    assert!(poll!(old.as_mut()).is_pending());
    assert!(matches!(
        terminal(&mut events).await,
        SamplingEvent::RequestCompleted { .. }
    ));
    handle.submit(id.clone(), request("replacement"));
    within(replacement.wait_blocked()).await;
    within(old).await.expect("old result was already delivered");
    cancel_current(&handle, &id, &mut events).await;
    within(owner.shutdown()).await.unwrap();
    assert_eq!(server.request_count(), 2);
}

#[tokio::test]
async fn plain_collect_return_does_not_cancel_replacement() {
    finished_collect_keeps_replacement(CollectEntry::Plain).await;
}

#[tokio::test]
async fn accounted_collect_return_does_not_cancel_replacement() {
    finished_collect_keeps_replacement(CollectEntry::Accounted).await;
}

async fn dropped_collect_keeps_replacement(entry: CollectEntry) {
    let server = within(MockInferenceServer::start()).await.unwrap();
    let mut first = expect_pending(&server, "old provider");
    let mut replacement = expect_pending(&server, "replacement provider");
    let (event_tx, mut events) = mpsc::unbounded_channel();
    let mut owner = SamplerActor::spawn_owned(config(&server), RetryPolicy::default(), event_tx);
    let handle = owner.handle();
    let id = RequestId::from("reused");
    let mut old = Box::pin(collect(entry, &handle, id.clone()));
    assert!(poll!(old.as_mut()).is_pending());
    within(first.wait_blocked()).await;
    handle.submit(id.clone(), request("replacement"));
    within(replacement.wait_blocked()).await;
    assert_cancelled(terminal(&mut events).await);
    drop(old);
    cancel_current(&handle, &id, &mut events).await;
    within(owner.shutdown()).await.unwrap();
    assert_eq!(server.request_count(), 2);
}

#[tokio::test]
async fn plain_collect_drop_does_not_cancel_replacement() {
    dropped_collect_keeps_replacement(CollectEntry::Plain).await;
}

#[tokio::test]
async fn accounted_collect_drop_does_not_cancel_replacement() {
    dropped_collect_keeps_replacement(CollectEntry::Accounted).await;
}

async fn dropped_collect_cancels_itself(entry: CollectEntry) {
    let server = within(MockInferenceServer::start()).await.unwrap();
    let mut provider = expect_pending(&server, "cancelled provider");
    let (event_tx, mut events) = mpsc::unbounded_channel();
    let mut owner = SamplerActor::spawn_owned(config(&server), RetryPolicy::default(), event_tx);
    let handle = owner.handle();
    let id = RequestId::from("owned");
    let mut pending = Box::pin(collect(entry, &handle, id.clone()));
    assert!(poll!(pending.as_mut()).is_pending());
    within(provider.wait_blocked()).await;
    drop(pending);
    assert_cancelled(terminal(&mut events).await);
    assert!(!within(handle.is_active(id)).await);
    within(owner.shutdown()).await.unwrap();
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn plain_collect_drop_cancels_its_own_provider() {
    dropped_collect_cancels_itself(CollectEntry::Plain).await;
}

#[tokio::test]
async fn accounted_collect_drop_cancels_its_own_provider() {
    dropped_collect_cancels_itself(CollectEntry::Accounted).await;
}

async fn dropped_before_actor_admission(entry: CollectEntry) {
    let server = within(MockInferenceServer::start()).await.unwrap();
    let (event_tx, mut events) = mpsc::unbounded_channel();
    let mut owner = SamplerActor::spawn_owned(config(&server), RetryPolicy::default(), event_tx);
    let handle = owner.handle();
    let id = RequestId::from("never-admitted");
    let mut pending = Box::pin(collect(entry, &handle, id.clone()));
    assert!(poll!(pending.as_mut()).is_pending());
    drop(pending);
    assert!(
        matches!(
            within(events.recv()).await,
            Some(SamplingEvent::Failed { error, .. }) if error.message.contains("cancelled")
        ),
        "no provider attempt may start after the owning future was dropped"
    );
    assert!(!within(handle.is_active(id)).await);
    within(owner.shutdown()).await.unwrap();
    assert_eq!(server.request_count(), 0);
}

#[tokio::test]
async fn plain_collect_drop_before_submit_processing_skips_provider() {
    dropped_before_actor_admission(CollectEntry::Plain).await;
}

#[tokio::test]
async fn accounted_collect_drop_before_submit_processing_skips_provider() {
    dropped_before_actor_admission(CollectEntry::Accounted).await;
}

#[tokio::test]
async fn panicked_request_reclaims_its_registration_and_actor_keeps_serving() {
    let server = within(MockInferenceServer::start()).await.unwrap();
    let _followup = expect_success(&server, "followup provider");
    let (event_tx, mut events) = mpsc::unbounded_channel();
    let mut owner = SamplerActor::spawn_owned(config(&server), RetryPolicy::default(), event_tx);
    let handle = owner.handle();
    let id = RequestId::from("panicked");
    let (sink, mut usages, _release) = usage_barrier();
    let capture: sampler::AttemptScopeCapture =
        Arc::new(|| Box::pin(async { panic!("controlled admission panic") }));
    let result = within(handle.submit_and_collect_accounted(
        id.clone(),
        request("panic"),
        Some(capture),
        Some(sink),
        None,
        sampler::RecoveryBudget::default(),
    ))
    .await;
    assert!(matches!(result, Err(SamplingError::Lifecycle(_))));
    assert!(!within(handle.is_active(id.clone())).await);
    assert_eq!(within(handle.active_count()).await, 0);
    within(handle.submit_and_collect(id, request("followup")))
        .await
        .unwrap();
    within(owner.shutdown()).await.unwrap();
    assert!(
        usages.try_recv().is_err(),
        "panic must not invent usage settlement"
    );
    let mut completed = 0;
    while let Some(event) = within(events.recv()).await {
        if matches!(event, SamplingEvent::RequestCompleted { .. }) {
            completed += 1;
        }
    }
    assert_eq!(completed, 1, "only the followup completed successfully");
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn replaced_task_panic_keeps_replacement_and_does_not_fabricate_settlement() {
    let server = within(MockInferenceServer::start()).await.unwrap();
    let _first = expect_success(&server, "old provider");
    let mut replacement = expect_pending(&server, "replacement provider");
    let (event_tx, mut events) = mpsc::unbounded_channel();
    let mut owner = SamplerActor::spawn_owned(config(&server), RetryPolicy::default(), event_tx);
    let handle = owner.handle();
    let id = RequestId::from("reused");
    let (entered_tx, mut entered_rx) = mpsc::unbounded_channel();
    let (release_tx, release_rx) = oneshot::channel();
    let release = Mutex::new(Some(release_rx));
    let evidence: sampler::audit::EvidenceSink = Arc::new(move |evidence| {
        let release = if evidence.kind == "response" {
            entered_tx.send(()).unwrap();
            release.lock().unwrap().take()
        } else {
            None
        };
        Box::pin(async move {
            if let Some(release) = release {
                release.await.unwrap();
                panic!("controlled response evidence panic");
            }
            Ok(())
        })
    });
    let (sink, mut usages, _usage_release) = usage_barrier();
    let mut old = Box::pin(handle.submit_and_collect_accounted(
        id.clone(),
        request("old"),
        None,
        Some(sink),
        Some(evidence),
        sampler::RecoveryBudget::default(),
    ));
    assert!(poll!(old.as_mut()).is_pending());
    assert_eq!(within(entered_rx.recv()).await, Some(()));
    handle.submit(id.clone(), request("replacement"));
    within(replacement.wait_blocked()).await;
    release_tx.send(()).unwrap();
    assert!(matches!(
        within(old).await,
        Err(SamplingError::Lifecycle(_))
    ));
    cancel_current(&handle, &id, &mut events).await;
    within(owner.shutdown()).await.unwrap();
    assert!(
        usages.try_recv().is_err(),
        "panic must not invent usage settlement"
    );
    assert_eq!(server.request_count(), 2);
}

#[tokio::test]
async fn normal_completion_settles_usage_once() {
    let server = within(MockInferenceServer::start()).await.unwrap();
    let _provider = expect_success(&server, "completed provider");
    let (event_tx, _events) = mpsc::unbounded_channel();
    let mut owner = SamplerActor::spawn_owned(config(&server), RetryPolicy::default(), event_tx);
    let handle = owner.handle();
    let (sink, mut usages, release) = usage_barrier();
    let mut pending = Box::pin(handle.submit_and_collect_accounted(
        RequestId::from("normal"),
        request("normal"),
        None,
        Some(sink),
        None,
        sampler::RecoveryBudget::default(),
    ));
    assert!(poll!(pending.as_mut()).is_pending());
    assert!(matches!(
        within(usages.recv()).await,
        Some(AttemptUsage::Known { .. })
    ));
    assert!(
        poll!(pending.as_mut()).is_pending(),
        "completion waits for usage ACK"
    );
    release.send(()).unwrap();
    within(pending).await.unwrap();
    within(owner.shutdown()).await.unwrap();
    assert!(usages.try_recv().is_err());
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn explicit_cancel_settles_usage_once() {
    let server = within(MockInferenceServer::start()).await.unwrap();
    let mut provider = expect_pending(&server, "cancelled provider");
    let (event_tx, mut events) = mpsc::unbounded_channel();
    let mut owner = SamplerActor::spawn_owned(config(&server), RetryPolicy::default(), event_tx);
    let handle = owner.handle();
    let id = RequestId::from("cancelled");
    let (sink, mut usages, release) = usage_barrier();
    let mut pending = Box::pin(handle.submit_and_collect_accounted(
        id.clone(),
        request("cancelled"),
        None,
        Some(sink),
        None,
        sampler::RecoveryBudget::default(),
    ));
    assert!(poll!(pending.as_mut()).is_pending());
    within(provider.wait_blocked()).await;
    handle.cancel(id);
    assert!(matches!(
        within(usages.recv()).await,
        Some(AttemptUsage::Incomplete { .. })
    ));
    assert!(
        poll!(pending.as_mut()).is_pending(),
        "cancel waits for usage ACK"
    );
    release.send(()).unwrap();
    assert!(matches!(
        within(pending).await,
        Err(SamplingError::Lifecycle(_))
    ));
    assert_cancelled(terminal(&mut events).await);
    within(owner.shutdown()).await.unwrap();
    assert!(usages.try_recv().is_err());
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn shutdown_drains_replaced_and_current_tasks_through_settlement_once() {
    let server = within(MockInferenceServer::start()).await.unwrap();
    let _first = expect_success(&server, "old provider");
    let mut replacement = expect_pending(&server, "replacement provider");
    let (event_tx, _events) = mpsc::unbounded_channel();
    let mut owner = SamplerActor::spawn_owned(config(&server), RetryPolicy::default(), event_tx);
    let handle = owner.handle();
    let id = RequestId::from("reused");
    let (old_sink, mut old_usages, old_release) = usage_barrier();
    let mut old = Box::pin(handle.submit_and_collect_accounted(
        id.clone(),
        request("old"),
        None,
        Some(old_sink),
        None,
        sampler::RecoveryBudget::default(),
    ));
    assert!(poll!(old.as_mut()).is_pending());
    assert!(matches!(
        within(old_usages.recv()).await,
        Some(AttemptUsage::Known { .. })
    ));
    let (new_sink, mut new_usages, new_release) = usage_barrier();
    let mut current = Box::pin(handle.submit_and_collect_accounted(
        id,
        request("replacement"),
        None,
        Some(new_sink),
        None,
        sampler::RecoveryBudget::default(),
    ));
    assert!(poll!(current.as_mut()).is_pending());
    within(replacement.wait_blocked()).await;
    let mut shutdown = Box::pin(owner.shutdown());
    assert!(poll!(shutdown.as_mut()).is_pending());
    assert!(matches!(
        within(new_usages.recv()).await,
        Some(AttemptUsage::Incomplete { .. })
    ));
    assert!(
        poll!(shutdown.as_mut()).is_pending(),
        "current settlement blocks shutdown"
    );
    new_release.send(()).unwrap();
    assert!(matches!(
        within(current).await,
        Err(SamplingError::Lifecycle(_))
    ));
    assert!(
        poll!(shutdown.as_mut()).is_pending(),
        "replaced settlement still blocks shutdown"
    );
    old_release.send(()).unwrap();
    within(shutdown).await.unwrap();
    assert!(matches!(
        within(old).await,
        Err(SamplingError::Lifecycle(_))
    ));
    assert!(old_usages.try_recv().is_err());
    assert!(new_usages.try_recv().is_err());
    assert_eq!(server.request_count(), 2);
}
