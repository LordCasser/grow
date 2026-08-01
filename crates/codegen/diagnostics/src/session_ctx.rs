//! Session-scoped structured diagnostics written through `tracing`.

use std::sync::Arc;

use serde::Serialize;

use crate::events::DiagnosticEvent;

#[derive(Clone)]
pub struct DiagnosticCtx {
    pub session_id: String,
    pub prompt_index: Arc<tokio::sync::Mutex<usize>>,
    pub prompt_id: Arc<parking_lot::Mutex<Option<String>>>,
}

impl DiagnosticCtx {
    pub fn new(session_id: String, prompt_index: Arc<tokio::sync::Mutex<usize>>) -> Self {
        Self {
            session_id,
            prompt_index,
            prompt_id: Arc::new(parking_lot::Mutex::new(None)),
        }
    }
}

tokio::task_local! {
    static DIAGNOSTIC_CTX: Arc<DiagnosticCtx>;
}

pub(crate) const SESSION_ID_FIELD: &str = "session_id";

fn session_span(session_id: &str) -> tracing::Span {
    tracing::info_span!("session", session_id = %session_id)
}

pub async fn with_session_ctx<F: std::future::Future>(ctx: DiagnosticCtx, fut: F) -> F::Output {
    use tracing::Instrument;
    let span = session_span(&ctx.session_id);
    DIAGNOSTIC_CTX
        .scope(Arc::new(ctx), fut.instrument(span))
        .await
}

pub fn begin_prompt_id() {
    let _ = DIAGNOSTIC_CTX.try_with(|ctx| {
        *ctx.prompt_id.lock() = Some(uuid::Uuid::new_v4().to_string());
    });
}

pub fn log_event<T: DiagnosticEvent>(data: T) {
    emit_event(T::NAME, data);
}

pub fn log_session_event<T: DiagnosticEvent>(data: T) {
    emit_event(T::NAME, data);
}

pub fn emit_event<T: Serialize + Send + 'static>(event: impl Into<String>, data: T) {
    let event = event.into();
    let payload = serde_json::to_value(data).unwrap_or(serde_json::Value::Null);
    let context = DIAGNOSTIC_CTX
        .try_with(|ctx| {
            (
                Some(ctx.session_id.clone()),
                ctx.prompt_index.try_lock().ok().map(|guard| *guard as u32),
                ctx.prompt_id.lock().clone(),
            )
        })
        .unwrap_or((None, None, None));
    tracing::info!(
        target: "diagnostics",
        diagnostic_event = %event,
        session_id = context.0.as_deref().unwrap_or(""),
        turn_number = context.1,
        prompt_id = context.2.as_deref().unwrap_or(""),
        payload = %payload,
        "structured diagnostic event"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_span_exposes_router_field() {
        let subscriber = tracing_subscriber::registry();
        tracing::subscriber::with_default(subscriber, || {
            let span = session_span("test-id");
            let meta = span.metadata().expect("enabled session span");
            assert!(meta.fields().field(SESSION_ID_FIELD).is_some());
        });
    }
}
