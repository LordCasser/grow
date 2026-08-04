//! TLS bootstrap helpers.
//!
//! reqwest 0.13 is built with `rustls-no-provider` (see
//! `docs/architecture/dependency-workarounds.md`): no crypto provider is
//! compiled into the binary, and the application must install one at runtime
//! before any TLS client is built. Production entrypoints (CLI, pager) install
//! the ring provider at startup; tests that build HTTP clients without going
//! through startup must call [`install_ring_provider_once`] too.

/// Install the ring crypto provider for rustls, once.
///
/// Idempotent: a second call (or an already-installed provider) is ignored.
/// `rustls::CryptoProvider::get_default()` stays `None` until this runs, and
/// reqwest panics when building a client without a default provider.
pub fn install_ring_provider_once() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}
