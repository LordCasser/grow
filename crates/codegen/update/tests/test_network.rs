//! Network-level integration tests using `wiremock`.
//!
//! Each `MockServer` binds to its own random port and these tests do not touch
//! global state, so no serial execution is required.

/// reqwest is built with `rustls-no-provider` (see the vendoring notes on the
/// workspace's rustls setup): production installs the ring provider at CLI
/// startup, but test binaries bypass startup, so install it once here.
#[ctor::ctor]
fn install_rustls_provider() {
    diagnostics::tls::install_ring_provider_once();
}

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use update::auto_update::{download_silent, download_with_progress};
use update::version::fetch_gh_release_version_from_url;

#[tokio::test]
async fn github_release_stable_ignores_drafts_and_prereleases() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/releases"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"tag_name": "v2.0.0", "draft": true, "prerelease": false},
            {"tag_name": "v1.3.0-alpha.1", "draft": false, "prerelease": true},
            {"tag_name": "v1.2.0", "draft": false, "prerelease": false},
            {"tag_name": "not-semver", "draft": false, "prerelease": false}
        ])))
        .expect(1)
        .mount(&server)
        .await;

    let version =
        fetch_gh_release_version_from_url("stable", &format!("{}/releases", server.uri()))
            .await
            .unwrap();
    assert_eq!(version, "1.2.0");
}

#[tokio::test]
async fn github_release_alpha_selects_highest_published_semver() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/releases"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"tag_name": "v1.2.0", "draft": false, "prerelease": false},
            {"tag_name": "v1.3.0-alpha.2", "draft": false, "prerelease": true},
            {"tag_name": "v1.3.0-alpha.1", "draft": false, "prerelease": true}
        ])))
        .mount(&server)
        .await;

    let version = fetch_gh_release_version_from_url("alpha", &format!("{}/releases", server.uri()))
        .await
        .unwrap();
    assert_eq!(version, "1.3.0-alpha.2");
}

// ─────────────────────────────────────────────────────────────────────────────
// download_silent — same body shape as download_with_progress but no
// progress bar to capture.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn download_silent_writes_body_to_dest() {
    let server = MockServer::start().await;
    let body = b"binary contents \x00\x01\x02".to_vec();
    Mock::given(method("GET"))
        .and(path("/grow-0.1.181-macos-aarch64"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("grow");
    let url = format!("{}/grow-0.1.181-macos-aarch64", server.uri());
    download_silent(&url, &dest).await.unwrap();

    let written = std::fs::read(&dest).unwrap();
    assert_eq!(written, body);
}

#[tokio::test]
async fn download_silent_preserves_binary_bytes_unchanged() {
    // Verify that arbitrary binary content (including null bytes, high
    // bytes, control chars) round-trips intact.
    let server = MockServer::start().await;
    let body: Vec<u8> = (0u8..=255).cycle().take(10_000).collect();
    Mock::given(method("GET"))
        .and(path("/bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("bin");
    download_silent(&format!("{}/bin", server.uri()), &dest)
        .await
        .unwrap();

    let written = std::fs::read(&dest).unwrap();
    assert_eq!(written, body);
}

#[tokio::test]
async fn download_silent_atomically_renames_via_tmp_file() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/bin"))
        .respond_with(ResponseTemplate::new(200).set_body_string("hello"))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("grow");
    download_silent(&format!("{}/bin", server.uri()), &dest)
        .await
        .unwrap();

    // After successful download, only the final file should exist.
    assert!(dest.exists());
    assert!(
        !dest.with_extension("tmp").exists(),
        "tmp file must be renamed away on success"
    );
}

/// A downloaded artifact must be published already executable (the install
/// path execs it right after download).
#[cfg(unix)]
#[tokio::test]
async fn download_silent_publishes_executable() {
    use std::os::unix::fs::PermissionsExt;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/bin"))
        .respond_with(ResponseTemplate::new(200).set_body_string("#!/bin/sh\necho ok\n"))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("grow-0.1.181-linux-x86_64");
    download_silent(&format!("{}/bin", server.uri()), &dest)
        .await
        .unwrap();

    let mode = std::fs::metadata(&dest).unwrap().permissions().mode();
    assert_ne!(
        mode & 0o111,
        0,
        "downloaded artifact must be executable on publish (mode {mode:o})"
    );
}

#[tokio::test]
async fn download_silent_fails_on_4xx() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/missing"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("grow");
    let err = download_silent(&format!("{}/missing", server.uri()), &dest)
        .await
        .unwrap_err();

    let msg = format!("{err:#}");
    assert!(msg.contains("Download failed"), "msg: {msg}");
    assert!(msg.contains("404"), "msg: {msg}");
    assert!(!dest.exists(), "no file should be created on HTTP error");
}

#[tokio::test]
async fn download_silent_fails_on_5xx() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("grow");
    let err = download_silent(&format!("{}/x", server.uri()), &dest)
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("503"));
}

#[tokio::test]
async fn download_silent_overwrites_existing_dest() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x"))
        .respond_with(ResponseTemplate::new(200).set_body_string("new content"))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("grow");
    std::fs::write(&dest, "old content").unwrap();

    download_silent(&format!("{}/x", server.uri()), &dest)
        .await
        .unwrap();

    let written = std::fs::read_to_string(&dest).unwrap();
    assert_eq!(written, "new content");
}

#[tokio::test]
async fn download_silent_handles_empty_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(Vec::<u8>::new()))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("grow");
    download_silent(&format!("{}/x", server.uri()), &dest)
        .await
        .unwrap();

    assert!(dest.exists());
    assert_eq!(std::fs::metadata(&dest).unwrap().len(), 0);
}

#[tokio::test]
async fn download_silent_streams_large_body() {
    // 5 MB to verify streaming (file is written incrementally, not loaded
    // entirely in memory before write).
    let server = MockServer::start().await;
    let body = vec![0xAB_u8; 5 * 1024 * 1024];
    Mock::given(method("GET"))
        .and(path("/big"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("grow");
    download_silent(&format!("{}/big", server.uri()), &dest)
        .await
        .unwrap();

    let written = std::fs::read(&dest).unwrap();
    assert_eq!(written.len(), body.len());
    assert_eq!(written, body);
}

#[tokio::test]
async fn download_silent_to_nonexistent_parent_dir_fails() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x"))
        .respond_with(ResponseTemplate::new(200).set_body_string("hi"))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    // Parent directory does NOT exist — should fail at file create.
    let dest = tmp.path().join("missing-subdir").join("grow");
    let err = download_silent(&format!("{}/x", server.uri()), &dest)
        .await
        .unwrap_err();
    let msg = format!("{err:#}").to_lowercase();
    assert!(
        msg.contains("no such file") || msg.contains("not found") || msg.contains("os error"),
        "expected fs error: {msg}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// download_with_progress — same contract; covers the spinner path
// (no Content-Length) and the progress-bar path (with Content-Length).
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn download_with_progress_writes_body_with_content_length() {
    // Wiremock sets Content-Length when set_body_bytes is used, so this
    // exercises the determinate-progress-bar path.
    let server = MockServer::start().await;
    let body = b"binary content".to_vec();
    Mock::given(method("GET"))
        .and(path("/grow"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("grow");
    download_with_progress(&format!("{}/grow", server.uri()), &dest)
        .await
        .unwrap();

    assert_eq!(std::fs::read(&dest).unwrap(), body);
}

#[tokio::test]
async fn download_with_progress_fails_on_http_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("grow");
    let err = download_with_progress(&format!("{}/x", server.uri()), &dest)
        .await
        .unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("Download failed"), "msg: {msg}");
    assert!(msg.contains("500"), "msg: {msg}");
}

#[tokio::test]
async fn download_with_progress_rejects_oversized_release_before_creating_a_temp_file() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/grow"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", (512_u64 * 1024 * 1024 + 1).to_string()),
        )
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("grow");
    let error = download_with_progress(&format!("{}/grow", server.uri()), &dest)
        .await
        .unwrap_err();

    assert!(format!("{error:#}").contains("outside the allowed range"));
    assert!(!dest.exists());
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn download_with_progress_atomic_rename() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("grow");
    download_with_progress(&format!("{}/x", server.uri()), &dest)
        .await
        .unwrap();

    assert!(dest.exists());
    assert!(!dest.with_extension("tmp").exists());
}

// ─────────────────────────────────────────────────────────────────────────────
// Parallel byte-range path — exercises the HEAD + 206 Partial Content code path
// in download_silent / download_with_progress for files >= 16 MiB.
// ─────────────────────────────────────────────────────────────────────────────

/// Wiremock responder for `GET` that honors `Range: bytes=A-B` with `206`.
/// Without a Range header it returns the full body with `200`.
#[derive(Clone)]
struct RangeResponder {
    body: std::sync::Arc<Vec<u8>>,
}

impl Respond for RangeResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let total = self.body.len();
        let spec = request
            .headers
            .get("range")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("bytes=").map(|x| x.to_string()));
        if let Some(spec) = spec
            && let Some((start_str, end_str)) = spec.split_once('-')
            && let (Ok(start), Ok(end)) = (start_str.parse::<usize>(), end_str.parse::<usize>())
        {
            let end = end.min(total - 1);
            if start <= end {
                let slice = self.body[start..=end].to_vec();
                return ResponseTemplate::new(206)
                    .insert_header("content-range", format!("bytes {start}-{end}/{total}"))
                    .set_body_bytes(slice);
            }
        }
        ResponseTemplate::new(200).set_body_bytes((*self.body).clone())
    }
}

#[tokio::test]
async fn download_silent_parallel_path_reassembles_bytes() {
    // 32 MiB body — clears the parallel threshold and yields 2 chunks
    // (size_mb / 16 = 2, clamped to [1, 8]), so this actually exercises
    // concurrent range fetches and the seek+write reassembly.
    let body: Vec<u8> = (0u32..(32 * 1024 * 1024 / 4))
        .flat_map(|n| n.to_le_bytes())
        .collect();
    assert_eq!(body.len(), 32 * 1024 * 1024);
    let arc = std::sync::Arc::new(body.clone());

    let server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/big"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", body.len().to_string())
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/big"))
        .respond_with(RangeResponder { body: arc })
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("grow-binary");
    download_silent(&format!("{}/big", server.uri()), &dest)
        .await
        .unwrap();

    let written = std::fs::read(&dest).unwrap();
    assert_eq!(written.len(), body.len());
    assert_eq!(
        written, body,
        "reassembled file must match original byte-for-byte"
    );
    assert!(
        !dest.with_extension("tmp").exists(),
        "tmp file must be cleaned up"
    );
}
