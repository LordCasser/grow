//! Sync deployment-key managed policy from the deployment-config endpoint.

mod response;

pub use response::ManagedConfigError;
use response::{ApplyOutcome, ManagedConfigResponse, ManagedConfigSource, verify_signed_envelope};

/// Server-synced policy artifacts. Excludes the sync marker ([`remove_managed_config_files`]
/// removes that last, only on full success).
pub const MANAGED_ARTIFACT_FILES: [&str; 4] = [
    grow_config::MANAGED_CONFIG_FILENAME,
    grow_config::REQUIREMENTS_FILENAME,
    grow_config::signed_policy::SIGNATURE_SIDECAR_FILE,
    grow_config::signed_policy::MANAGED_IDENTITY_SIDECAR_FILE,
];

/// Delete server-synced files then the marker (never `config.toml`).
fn remove_managed_config_files(home: &std::path::Path) {
    let mut artifacts_removed = true;
    for name in MANAGED_ARTIFACT_FILES {
        artifacts_removed &= remove_synced_file(home, name, "removed managed config file");
    }
    // Marker last, only on full success: crash/error leaves the detector armed for the next start.
    if artifacts_removed {
        remove_synced_file(
            home,
            grow_config::MANAGED_CONFIG_CACHE_FILE,
            "removed managed config file",
        );
    }
    // Best-effort sweep of mid-write `.tmp` leftovers (a concurrent writer's temp may go too —
    // its rename fails and self-heals).
    let atomic_write_tmp_prefixes = [
        format!("{}.", grow_config::MANAGED_CONFIG_CACHE_FILE),
        format!("{}.", grow_config::signed_policy::SIGNATURE_SIDECAR_FILE),
        format!(
            "{}.",
            grow_config::signed_policy::MANAGED_IDENTITY_SIDECAR_FILE
        ),
    ];
    if let Ok(entries) = std::fs::read_dir(home) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let is_write_tmp = name.ends_with(".tmp")
                && atomic_write_tmp_prefixes
                    .iter()
                    .any(|prefix| name.starts_with(prefix.as_str()));
            if is_write_tmp {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Returns whether the path is gone (removed or already absent); `false` = removal failed.
fn remove_synced_file(home: &std::path::Path, name: &str, why: &str) -> bool {
    let path = home.join(name);
    match remove_managed_path(&path) {
        Ok(true) => {
            tracing::info!(file = %path.display(), "{why}");
            true
        }
        Ok(false) => true,
        Err(e) => {
            tracing::warn!(file = %path.display(), error = %e, "failed to remove managed config file");
            false
        }
    }
}

/// Clear a directory squatting where a managed file is about to be WRITTEN — the atomic
/// rename would fail onto it forever, permanently blocking the self-heal. Best-effort:
/// the write's own error surfaces if clearing fails.
fn clear_squatting_dir(path: &std::path::Path) {
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
        && let Err(e) = remove_managed_path(path)
    {
        tracing::warn!(error = %e, "failed to clear a directory squatting at a managed config path");
    }
}

/// Remove whatever occupies a managed artifact path — a squatting DIRECTORY too, else a
/// dir-squat would block removal and rewrite forever. Only ever called with the fixed
/// managed artifact/marker/sidecar names. `Ok(true)` = removed; `Ok(false)` = already absent.
fn remove_managed_path(path: &std::path::Path) -> std::io::Result<bool> {
    let is_dir = std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir());
    let result = if is_dir {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    match result {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// Remove cached managed policy when no deployment key owns it. Fail-closed
/// artifacts remain on disk so removing a key cannot bypass enforced policy.
pub fn clear_orphan() {
    if resolve_deployment_key().is_some() {
        return;
    }
    let home = crate::util::grow_home::grow_home();
    let Some(_lock) = try_lock_managed_config(&home) else {
        return;
    };
    if grow_config::fail_closed_policy_armed_at(&home) {
        tracing::info!("keeping fail_closed managed policy after deployment key removal");
        return;
    }
    remove_managed_config_files(&home);
}

/// Best-effort cross-process lock serializing apply/remove of the managed-config
/// files (TUI tick vs `grow login` vs prefetch). `None` on contention — the
/// caller skips and retries next cycle.
fn try_lock_managed_config(home: &std::path::Path) -> Option<std::fs::File> {
    use fs2::FileExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(home.join("managed_config.lock"))
        .ok()?;
    file.try_lock_exclusive().ok()?;
    Some(file)
}

/// Retry budget for a sync, pairing the attempt count with a wall-clock cap.
#[derive(Clone, Copy)]
enum SyncBudget {
    /// Background loop and explicit `grow setup`; runs retries to completion.
    Standard,
    /// Session-start refresh; capped so startup never stalls.
    SessionStart,
}

impl SyncBudget {
    /// Total fetch attempts (first try included) for transient failures.
    fn max_attempts(self) -> u32 {
        match self {
            Self::Standard => 5,
            Self::SessionStart => 2,
        }
    }

    /// Wall-clock cap, or `None` to let retries run to completion.
    fn deadline(self) -> Option<std::time::Duration> {
        match self {
            Self::Standard => None,
            Self::SessionStart => Some(std::time::Duration::from_secs(8)),
        }
    }
}

/// Exponential backoff for retry `attempt` (caller guarantees `attempt >= 1`).
/// Base is 1s; `GROW_DEPLOYMENT_CONFIG_BACKOFF_MS` overrides it for tests.
fn retry_backoff(attempt: u32) -> std::time::Duration {
    let base = std::env::var("GROW_DEPLOYMENT_CONFIG_BACKOFF_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(1000);
    std::time::Duration::from_millis(base << attempt.saturating_sub(1))
}

/// Fetch the managed-config response, retrying transient (network / connection
/// interruption / 5xx) failures with exponential backoff. Auth errors fail
/// immediately, mapped via `source` so the message names the rejected credential.
///
/// Routes the whole once-fetch (send + body read + decode) through `crate::http::send_with_retry_escaping_pool`,
/// so a body-phase interruption is retried (not just the send) and the final attempt escapes a
/// poisoned pool on a fresh connection (see that helper for the escape policy).
async fn fetch_managed_config(
    url: &str,
    token: &str,
    source: ManagedConfigSource,
    max_attempts: u32,
    echo_principal: Option<&str>,
) -> Result<ManagedConfigResponse, ManagedConfigError> {
    crate::http::send_with_retry_escaping_pool(
        move |client: reqwest::Client| async move {
            fetch_managed_config_once(&client, url, token, source, echo_principal).await
        },
        max_attempts,
        |e: &ManagedConfigError| e.is_retryable(),
        |attempt| tokio::time::sleep(retry_backoff(attempt)),
    )
    .await
}

/// Persist a fetched response under `home`, converging disk to the served set: served
/// artifacts are overwritten, unserved ones removed — a leftover must not keep enforcing
/// a withdrawn policy or trip the signed absence check. Returns whether anything changed.
fn apply_managed_config(
    home: &std::path::Path,
    body: &ManagedConfigResponse,
) -> std::io::Result<bool> {
    use crate::util::config::atomic_write_string;

    let artifacts = [
        (
            grow_config::MANAGED_CONFIG_FILENAME,
            body.managed_config.as_deref(),
        ),
        (
            grow_config::REQUIREMENTS_FILENAME,
            body.requirements.as_deref(),
        ),
    ];

    let mut changed = false;
    let mut first_err: Option<std::io::Error> = None;
    for (name, content) in artifacts {
        let path = home.join(name);
        match content.filter(|s| !s.is_empty()) {
            Some(content) => {
                clear_squatting_dir(&path);
                match atomic_write_string(&path, content) {
                    Ok(()) => changed = true,
                    Err(e) => {
                        first_err.get_or_insert(e);
                    }
                }
            }
            None => match remove_managed_path(&path) {
                Ok(true) => {
                    tracing::info!("removed managed config artifact the server no longer serves");
                    changed = true;
                }
                Ok(false) => {}
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            },
        }
    }

    if changed {
        tracing::info!("managed config refreshed from server");
    }
    match first_err {
        Some(e) => Err(e),
        None => Ok(changed),
    }
}

/// Map a classified transport failure to a `ManagedConfigError`. Split out from [`map_send_error`]
/// so the mapping (and its retryability) is unit-testable by constructing `TransportFailure` directly.
fn map_transport_failure(failure: crate::http::TransportFailure) -> ManagedConfigError {
    use crate::http::TransportFailureKind;
    match failure.kind {
        TransportFailureKind::Unreachable => ManagedConfigError::Network(failure.detail),
        TransportFailureKind::Interrupted => {
            ManagedConfigError::ConnectionInterrupted(failure.detail)
        }
        // A builder/redirect failure is a client-side defect, not a bad server response: terminal.
        TransportFailureKind::Permanent => ManagedConfigError::RequestFailed(failure.detail),
    }
}

/// Map a `reqwest` send failure to a `ManagedConfigError` via the shared `grow-http` classifier.
fn map_send_error(e: &reqwest::Error) -> ManagedConfigError {
    map_transport_failure(crate::http::TransportFailure::classify(e))
}

async fn fetch_managed_config_once(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    source: ManagedConfigSource,
    echo_principal: Option<&str>,
) -> Result<ManagedConfigResponse, ManagedConfigError> {
    let mut request = client
        .get(url)
        .header("Authorization", format!("Bearer {}", token))
        .timeout(std::time::Duration::from_secs(15));
    // Replay-probe echo (diagnostics only). Skip on invalid HeaderValue so a
    // corrupt sidecar never bricks the fetch (echo is fail-open).
    if let Some(nonce) = grow_config::signed_policy::stored_envelope_nonce(
        &crate::util::grow_home::grow_home(),
        echo_principal,
    ) && let Ok(value) = reqwest::header::HeaderValue::from_str(&nonce)
    {
        request = request.header(
            grow_config::signed_policy::MANAGED_CONFIG_NONCE_ECHO_HEADER,
            value,
        );
    }
    let resp = match request.send().await {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            let status = r.status().as_u16();
            tracing::debug!(status, "managed config fetch failed");
            return Err(if status == 401 || status == 403 {
                source.auth_rejected_error()
            } else {
                ManagedConfigError::ServerError { status }
            });
        }
        Err(e) => {
            let err = map_send_error(&e);
            tracing::debug!(error = %err, "managed config fetch error");
            return Err(err);
        }
    };

    // Split the body read from the decode so the FAILING OPERATION disambiguates transport from
    // payload: reqwest tags both a mid-body connection drop and malformed JSON as `Kind::Decode`
    // from `json()`, so reading raw `bytes()` first (any error there is an in-flight transport
    // interruption, retryable) then `from_slice` (any error there is a malformed payload, terminal)
    // avoids fragile error-kind/source inspection.
    let bytes = match resp.bytes().await {
        Ok(b) => b,
        // A body-read failure is an in-flight transport interruption, so it is retryable.
        Err(e) => {
            return Err(ManagedConfigError::ConnectionInterrupted(
                crate::http::error_cause_chain(&e),
            ));
        }
    };
    serde_json::from_slice::<ManagedConfigResponse>(&bytes)
        .map_err(|e| ManagedConfigError::InvalidResponse(e.to_string()))
}

/// Override with `GROW_DEPLOYMENT_CONFIG_REFRESH_INTERVAL_SECS`. Clamped to
/// >= 1s: `tokio::time::interval` panics on a zero period.
fn managed_config_sync_interval() -> std::time::Duration {
    if let Ok(s) = std::env::var("GROW_DEPLOYMENT_CONFIG_REFRESH_INTERVAL_SECS")
        && let Ok(secs) = s.parse::<u64>()
    {
        return std::time::Duration::from_secs(secs.max(1));
    }
    std::time::Duration::from_secs(5 * 60)
}

/// Periodically sync managed config in the background. Best-effort.
pub fn spawn_sync(cancel: tokio_util::sync::CancellationToken) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(managed_config_sync_interval());
        interval.tick().await; // skip immediate first tick

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = interval.tick() => {}
            }

            // Clear a logged-out team's files before deciding to fetch, so
            // stale enforced policy never outlives the tick.
            clear_orphan();
            // Raise the floor each tick so a long offline session keeps recording
            // observed time; otherwise a later rollback could make an expired policy
            // read valid.
            bump_managed_rollback_floor();

            if !crate::config::is_managed_config_stale_for(&current_serving_identity())
                || !is_fetch_enabled()
            {
                continue;
            }

            match sync().await {
                Ok(true) => tracing::info!("background managed config sync: updated"),
                Ok(false) => {}
                Err(e) => tracing::debug!("background managed config sync failed: {e}"),
            }
        }

        tracing::debug!("managed config sync task stopped");
    });
}

/// Deployment id associated with `deployment_key` in credential snapshots:
/// the server GrowDeployment UUID when the managed-config sync marker was
/// written by this same key (fingerprint match), else UUIDv5 of the key.
/// A missing key returns `None`, never a stale marker value.
pub fn resolve_deployment_id(deployment_key: Option<&str>) -> Option<String> {
    let key = deployment_key.filter(|k| !k.is_empty())?;
    crate::config::managed_deployment_id(&deployment_key_fingerprint(key))
        .or_else(|| Some(crate::agent::config::deployment_id_from_key(key)))
}

/// Resolve deployment key from `GROW_DEPLOYMENT_KEY` env var, then config files.
pub fn resolve_deployment_key() -> Option<String> {
    let config_val = crate::config::load_effective_config()
        .map_err(|e| tracing::warn!("failed to load config files for deployment key: {e}"))
        .ok()
        .and_then(|root| {
            root.get("endpoints")?
                .get("deployment_key")?
                .as_str()
                .map(|s| s.to_owned())
        });
    crate::agent::config::resolve_string_flag(
        None,
        "GROW_DEPLOYMENT_KEY",
        config_val.as_deref(),
        None,
    )
    .map(|r| r.value)
}

/// One-way blake3 fingerprint of a deployment key — the deploy-key identity (see [`crate::config::ServingIdentity`]).
/// Deterministic so the same key matches its marker; the raw key is never written to disk.
fn deployment_key_fingerprint(key: &str) -> String {
    blake3::hash(key.as_bytes()).to_hex().to_string()
}

/// Whether managed config fetching is enabled (env > config.toml > default true).
/// Callers doing auto-fetch should check this; explicit user actions (grow setup) skip it.
pub fn is_fetch_enabled() -> bool {
    if let Some(v) = crate::agent::config::env_bool("GROW_MANAGED_CONFIG") {
        return v;
    }
    crate::config::load_effective_config()
        .ok()
        .and_then(|cfg| cfg.get("features")?.get("managed_config")?.as_bool())
        .unwrap_or(true)
}

/// Fetch managed config + requirements for the configured deployment key.
pub async fn sync() -> Result<bool, ManagedConfigError> {
    Ok(sync_with_budget(SyncBudget::Standard).await?.wrote)
}

struct SyncOutcome {
    wrote: bool,
    served: bool,
    skipped: bool,
    signature_rejected: bool,
}

impl SyncOutcome {
    fn from_fetch(body: &ManagedConfigResponse, outcome: &ApplyOutcome) -> Self {
        Self {
            wrote: outcome.wrote(),
            served: body.config_exists(),
            skipped: outcome.skipped(),
            signature_rejected: outcome.signature_rejected(),
        }
    }
}

async fn sync_bounded(budget: SyncBudget) -> Option<Result<SyncOutcome, ManagedConfigError>> {
    let sync = sync_with_budget(budget);
    match budget.deadline() {
        Some(deadline) => tokio::time::timeout(deadline, sync).await.ok(),
        None => Some(sync.await),
    }
}

enum FetchedConfig {
    DeploymentKey {
        key: String,
        body: ManagedConfigResponse,
    },
    NoPrincipal,
}

async fn fetch_for_principal(budget: SyncBudget) -> Result<FetchedConfig, ManagedConfigError> {
    let Some(key) = resolve_deployment_key() else {
        return Ok(FetchedConfig::NoPrincipal);
    };
    let url = crate::agent::config::EndpointsConfig::from_effective_config()
        .resolve_managed_config_url()
        .ok_or_else(|| {
            ManagedConfigError::RequestFailed(
                "managed configuration requires endpoints.managed_config_url or endpoints.cli_chat_proxy_base_url"
                    .to_owned(),
            )
        })?;
    let echo_principal = crate::config::managed_deployment_id(&deployment_key_fingerprint(&key));
    let body = fetch_managed_config(
        &url,
        &key,
        ManagedConfigSource::DeploymentKey,
        budget.max_attempts(),
        echo_principal.as_deref(),
    )
    .await?;
    Ok(FetchedConfig::DeploymentKey { key, body })
}

async fn sync_with_budget(budget: SyncBudget) -> Result<SyncOutcome, ManagedConfigError> {
    match fetch_for_principal(budget).await? {
        FetchedConfig::DeploymentKey { key, body } => {
            let fingerprint = deployment_key_fingerprint(&key);
            let outcome = apply_fetched(&body, body.deployment_id.as_deref(), Some(&fingerprint))?;
            Ok(SyncOutcome::from_fetch(&body, &outcome))
        }
        FetchedConfig::NoPrincipal => Ok(SyncOutcome {
            wrote: false,
            served: false,
            skipped: false,
            signature_rejected: false,
        }),
    }
}

/// Apply under the cross-process lock (`Skipped` if contended — holder's sync supersedes).
/// `new_principal` / `new_key_fingerprint` are the serving identity for pre-write eviction.
fn apply_fetched(
    body: &ManagedConfigResponse,
    new_principal: Option<&str>,
    new_key_fingerprint: Option<&str>,
) -> std::io::Result<ApplyOutcome> {
    // Verify before lock/persist: prior trusted policy survives a bad fetch. Pure so a
    // lock-skip never reports Applied for an envelope that would have failed.
    let verified = if grow_config::signed_policy::verification_active() {
        match verify_signed_envelope(body) {
            Ok(verified) => Some(verified),
            Err(e) => {
                tracing::warn!("managed config signature rejected; not persisting: {e}");
                return Ok(ApplyOutcome::SignatureRejected);
            }
        }
    } else {
        None
    };
    let signed_deployment_id = verified
        .as_ref()
        .and_then(|v| v.payload.deployment_id.clone());
    let home = crate::util::grow_home::grow_home();
    let Some(_lock) = try_lock_managed_config(&home) else {
        tracing::debug!("managed config locked by another process; skipping apply");
        return Ok(ApplyOutcome::Skipped);
    };
    // The deployment key may have vanished mid-fetch; do not restore policy
    // whose owner is no longer configured.
    if resolve_deployment_key().is_none() {
        tracing::info!("deployment key gone since fetch started; skipping apply");
        return Ok(ApplyOutcome::Skipped);
    }
    // Confirmed switch: evict first so omitted artifacts from the prior principal don't stick.
    // Same locked `home` as the flock + marker write (no re-resolve).
    if crate::config::managed_config_identity_changed_at(&home, new_principal, new_key_fingerprint)
    {
        evict_prior_managed_config(&home);
    }
    let wrote = apply_managed_config(&home, body)?;
    // Sidecar after policy files so a present sidecar covers the final set; clear dir squats
    // that would fail the atomic rename forever.
    if let Some(verified) = verified {
        clear_squatting_dir(&home.join(grow_config::signed_policy::SIGNATURE_SIDECAR_FILE));
        grow_config::signed_policy::write_sidecar(&home, &verified.sidecar)?;
        // Disk errors are fatal, like the policy sidecar's.
        if let Some(claim_sidecar) =
            verified_claim_sidecar(body, served_principal_of(&verified.payload))
        {
            clear_squatting_dir(
                &home.join(grow_config::signed_policy::MANAGED_IDENTITY_SIDECAR_FILE),
            );
            grow_config::signed_policy::write_managed_identity_sidecar(&home, &claim_sidecar)?;
        }
    }
    // Marker last, still under the lock: written post-release, a concurrent purge could
    // delete the files it describes. A squatting dir would fail the atomic rename forever.
    clear_squatting_dir(&home.join(grow_config::MANAGED_CONFIG_CACHE_FILE));
    crate::config::mark_managed_config_synced_at(
        &home,
        crate::config::SyncMarker {
            principal: signed_deployment_id.as_deref().or(new_principal),
            had_managed_config: body.has_managed_config(),
            had_requirements: body.has_requirements(),
            key_fingerprint: new_key_fingerprint,
            fail_closed: body.requirements_fail_closed(),
        },
    );
    Ok(ApplyOutcome::Applied { wrote })
}

/// The deployment principal a verified payload binds.
fn served_principal_of(payload: &grow_config::signed_policy::SignedPayload) -> Option<&str> {
    payload.deployment_id.as_deref()
}

/// The fetched claim envelope, if it verifies and binds to the served principal.
/// `None` skips (old server / unverifiable / foreign): a bad claim must not fail
/// the apply — it only hardens the policy sidecar.
fn verified_claim_sidecar(
    body: &ManagedConfigResponse,
    served_principal: Option<&str>,
) -> Option<grow_config::signed_policy::SignatureEnvelope> {
    use grow_config::signed_policy::now_unix;
    let sidecar = body.managed_identity_sidecar()?;
    // Unclamped wall clock, like the policy verify: a fresh claim heals an inflated floor.
    let claim = match grow_config::signed_policy::verify_fetched_claim(&sidecar, now_unix()) {
        Ok(claim) => claim,
        Err(e) => {
            tracing::debug!("is-managed claim did not verify; not persisting it: {e}");
            return None;
        }
    };
    if !claim_binds_to(&claim, served_principal) {
        tracing::debug!("is-managed claim is bound to a different principal; not persisting it");
        return None;
    }
    Some(sidecar)
}

/// The persist rule: a verified claim persists only when bound to the served principal.
fn claim_binds_to(
    claim: &grow_config::signed_policy::ManagedIdentityClaim,
    served_principal: Option<&str>,
) -> bool {
    served_principal == Some(claim.principal.as_str())
}

/// Evict the prior principal's policy artifacts on a confirmed switch; this apply then
/// writes the new set and rebinds the marker. Includes the sidecars — a verification-inactive
/// build must not leave the prior tenant's sidecar to read foreign-bound on a signing build.
fn evict_prior_managed_config(home: &std::path::Path) {
    for name in MANAGED_ARTIFACT_FILES {
        remove_synced_file(home, name, "evicted prior principal's artifact");
    }
}

/// Whether a deployment key exists that `grow setup` can use.
pub fn has_principal() -> bool {
    resolve_deployment_key().is_some()
}

fn managed_principal_present() -> bool {
    resolve_deployment_key().is_some()
}

/// Deployment-key identity used by cache and fail-closed checks.
pub fn current_serving_identity() -> crate::config::ServingIdentity {
    match resolve_deployment_key() {
        Some(key) => crate::config::ServingIdentity::DeploymentKey {
            fingerprint: deployment_key_fingerprint(&key),
        },
        None => crate::config::ServingIdentity::None,
    }
}

/// Best-effort bounded refresh for a hard-stale deployment-key policy.
pub async fn ensure_managed_policy_present() {
    if !is_fetch_enabled() || !has_principal() {
        return;
    }
    let identity = current_serving_identity();
    if !crate::config::is_managed_config_hard_stale_for(&identity) {
        return;
    }
    match sync_bounded(SyncBudget::SessionStart).await {
        Some(Ok(_)) => {}
        Some(Err(e)) => tracing::warn!("session-start managed policy refresh failed: {e}"),
        None => tracing::warn!("session-start managed policy refresh timed out"),
    }
}

/// Shown when a managed principal's enforced policy is missing/substituted and the refetch couldn't restore it.
const MANAGED_POLICY_MISSING_MSG: &str = "Managed policy is required for this account but is \
missing or could not be verified, and could not be restored from the server.\nThis check needs \
network access: reconnect and start again. If you can't reconnect, contact your administrator.";

/// Fail-closed session-start gate for deployment-key managed policy.
/// Without a signing key the user-writable marker is best-effort; root/MDM/signed cache
/// are the non-forgeable layers. Recovery: reconnect / `grow setup`; ceasing to serve
/// `fail_closed` rolls back.
pub fn managed_policy_gate() -> Result<(), String> {
    // Lib unit tests skip: bootstrap would hit the host's real marker. Pure decision
    // is unit-tested; integration tests exercise this path.
    if cfg!(test) {
        return Ok(());
    }
    bump_managed_rollback_floor();
    managed_policy_gate_decision(
        managed_principal_present(),
        crate::config::managed_policy_compromised_for(&current_serving_identity()),
    )
}

/// Floor tick (session start + background sync tick), best-effort under the
/// managed-config lock — a failed tick must not refuse a session.
fn bump_managed_rollback_floor() {
    // Re-checked inside `bump_rollback_floor`; this early-out skips the lock I/O when dark.
    if !grow_config::signed_policy::verification_active() {
        return;
    }
    let home = crate::util::grow_home::grow_home();
    match try_lock_managed_config(&home) {
        Some(_lock) => {
            grow_config::bump_rollback_floor(&home);
        }
        None => tracing::debug!("managed-config lock contended; skipping the floor tick"),
    }
}

/// Pure decision behind [`managed_policy_gate`]: fail closed only when a managed principal is active AND its policy is compromised.
fn managed_policy_gate_decision(
    managed_principal_present: bool,
    policy_compromised: bool,
) -> Result<(), String> {
    if managed_principal_present && policy_compromised {
        return Err(MANAGED_POLICY_MISSING_MSG.to_string());
    }
    Ok(())
}

/// Outcome of the `grow setup` sync. The caller renders it — CLI presentation
/// and exit codes stay out of the library.
#[derive(Debug)]
pub enum SetupOutcome {
    /// Config was written to `~/.grow`.
    Installed,
    /// The principal is valid but the server has no config for it.
    NothingConfigured,
    /// Nothing persisted by THIS run (another process held the apply lock, or the credential
    /// vanished mid-fetch); re-running converges.
    Skipped,
    /// The fetch failed.
    Failed(ManagedConfigError),
}

/// Result of `grow setup --json`: what the server serves for the current
/// principal, verbatim. `managed_config` may embed the enforced deployment key,
/// exactly as `grow setup` would write it to disk.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupReport {
    /// `"deploymentKey"` when a principal was available.
    pub source: Option<&'static str>,
    /// Whether the server has a configuration for the principal.
    pub configured: bool,
    pub deployment_id: Option<String>,
    /// TOML documents exactly as `grow setup` would install them.
    pub managed_config: Option<String>,
    pub requirements: Option<String>,
    pub fail_closed: bool,
}

/// Fetches the report behind `grow setup --json` without writing anything:
/// no artifacts, no signature sidecar, no sync marker.
pub async fn fetch_setup_report() -> Result<SetupReport, ManagedConfigError> {
    let (source, body) = match fetch_for_principal(SyncBudget::Standard).await? {
        FetchedConfig::DeploymentKey { body, .. } => (Some("deploymentKey"), body),
        FetchedConfig::NoPrincipal => (None, ManagedConfigResponse::default()),
    };
    // Match the installer's trust decision: a payload `grow setup` would refuse
    // is reported as an error, not printed as installable config.
    if source.is_some()
        && grow_config::signed_policy::verification_active()
        && let Err(e) = verify_signed_envelope(&body)
    {
        tracing::warn!("managed config signature rejected: {e}");
        return Err(ManagedConfigError::SignatureRejected);
    }
    Ok(SetupReport {
        source,
        configured: body.config_exists(),
        fail_closed: body.requirements_fail_closed(),
        deployment_id: body.deployment_id,
        managed_config: body.managed_config,
        requirements: body.requirements,
    })
}

/// Run the `grow setup` sync for the current principal. The caller must check
/// [`has_principal`] first and render the no-principal guidance.
pub async fn run_setup() -> SetupOutcome {
    match sync_with_budget(SyncBudget::Standard).await {
        // A rejected envelope persisted nothing — reporting Installed would mask a
        // fetch the gate is about to refuse.
        Ok(SyncOutcome {
            signature_rejected: true,
            ..
        }) => SetupOutcome::Failed(ManagedConfigError::SignatureRejected),
        // A skip persisted nothing: not Installed (this run wrote nothing) nor NothingConfigured
        // (the server does have config).
        Ok(SyncOutcome { skipped: true, .. }) => SetupOutcome::Skipped,
        // `served` (not `wrote`) so an unchanged re-fetch isn't reported as "no config".
        Ok(SyncOutcome { served: true, .. }) => SetupOutcome::Installed,
        Ok(_) => SetupOutcome::NothingConfigured,
        Err(e) => SetupOutcome::Failed(e),
    }
}

// Tests in a sibling file (they dwarf the module) but a child module, for private access.
#[cfg(test)]
#[path = "managed_config/tests.rs"]
mod tests;
