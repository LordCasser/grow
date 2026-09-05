//! Provider evidence, scoped to one attempt. Owners persist records in their
//! existing causal ledger; this module has no filesystem or replay authority.

use futures_util::future::BoxFuture;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

pub const MAX_RESPONSE_EVIDENCE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug)]
pub struct Evidence {
    pub kind: &'static str,
    pub metadata: Value,
    pub body: Arc<Vec<u8>>,
}

pub type EvidenceSink =
    Arc<dyn Fn(Evidence) -> BoxFuture<'static, Result<(), String>> + Send + Sync>;

#[derive(Default)]
struct Capture {
    response: Arc<Vec<u8>>,
    status: Option<u16>,
    overflow: bool,
    failure: Option<String>,
    dispatched: bool,
}

#[derive(Clone)]
pub struct AttemptEvidence {
    sink: EvidenceSink,
    capture: Arc<Mutex<Capture>>,
}

tokio::task_local! { static CURRENT: AttemptEvidence; }

impl AttemptEvidence {
    pub fn new(sink: EvidenceSink) -> Self {
        Self {
            sink,
            capture: Arc::new(Mutex::new(Capture::default())),
        }
    }

    pub async fn scope<F: std::future::Future>(&self, future: F) -> F::Output {
        CURRENT.scope(self.clone(), future).await
    }

    pub(crate) fn current() -> Option<Self> {
        CURRENT.try_with(Clone::clone).ok()
    }

    pub(crate) fn response(&self, bytes: &[u8]) -> Result<(), String> {
        let mut capture = self.capture.lock().expect("evidence capture poisoned");
        let remaining = MAX_RESPONSE_EVIDENCE_BYTES.saturating_sub(capture.response.len());
        Arc::make_mut(&mut capture.response)
            .extend_from_slice(&bytes[..bytes.len().min(remaining)]);
        capture.overflow |= bytes.len() > remaining;
        if capture.overflow {
            return Err("provider response evidence exceeds 64 MiB".into());
        }
        Ok(())
    }

    /// Whether the HTTP execute future was admitted, independently of a
    /// logical accounting lease captured before the evidence barrier.
    pub fn was_dispatched(&self) -> bool {
        self.capture
            .lock()
            .expect("evidence capture poisoned")
            .dispatched
    }

    pub(crate) fn mark_dispatched(&self) {
        self.capture
            .lock()
            .expect("evidence capture poisoned")
            .dispatched = true;
    }

    pub(crate) fn status(&self, status: u16) {
        self.capture
            .lock()
            .expect("evidence capture poisoned")
            .status = Some(status);
    }

    /// ACK gates HTTP emission. Credentials and request headers are deliberately
    /// outside this evidence payload; the body is the exact encoded projection.
    pub(crate) async fn request(
        &self,
        body: &[u8],
        backend: sampling_types::ApiBackend,
        route: String,
    ) -> Result<(), String> {
        let result = (self.sink)(Evidence {
            kind: "request",
            metadata: json!({"backend": backend, "route": route}),
            body: Arc::new(body.to_vec()),
        })
        .await;
        if let Err(error) = &result {
            self.capture
                .lock()
                .expect("evidence capture poisoned")
                .failure = Some(error.clone());
        }
        result
    }

    /// Called before accepting, discarding, or retrying an attempt. The stream
    /// captures raw bytes before BOM removal, SSE decoding or extension filters.
    /// A bounded prefix is retained on overflow and the attempt fails closed.
    pub async fn finish(&self, metadata: Value) -> Result<(), String> {
        let (body, status, overflow, failure) = {
            let capture = self.capture.lock().expect("evidence capture poisoned");
            (
                capture.response.clone(),
                capture.status,
                capture.overflow,
                capture.failure.clone(),
            )
        };
        if let Some(error) = failure {
            return Err(error);
        }
        (self.sink)(Evidence {
            kind: "response",
            metadata: json!({"status": status, "truncated_by_evidence_limit": overflow, "outcome": metadata}),
            body,
        }).await?;
        self.capture
            .lock()
            .expect("evidence capture poisoned")
            .response = Arc::default();
        if overflow {
            return Err("provider response evidence exceeds 64 MiB".into());
        }
        Ok(())
    }
}

pub async fn record(
    sink: Option<&EvidenceSink>,
    kind: &'static str,
    metadata: Value,
) -> Result<(), String> {
    if let Some(sink) = sink {
        sink(Evidence {
            kind,
            metadata,
            body: Arc::default(),
        })
        .await?;
    }
    Ok(())
}
