//! Cross-platform transport shared by Grow's local IPC protocols.
//!
//! - **Unix:** wraps Tokio Unix sockets and restricts the socket to its owner.
//! - **Windows:** wraps `tokio::net::windows::named_pipe::*` (tokio doesn't
//!   expose AF_UNIX on Windows). The logical filesystem path is hashed
//!   into `\\.\pipe\grow-local-<hash>` so callers keep their path-based API.
//!
#[cfg(unix)]
mod unix_impl {
    use std::io;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    pub use tokio::net::UnixStream as LocalStream;

    pub struct LocalListener {
        inner: tokio::net::UnixListener,
    }

    impl LocalListener {
        pub fn bind<P: AsRef<Path>>(path: P) -> io::Result<Self> {
            let path = path.as_ref();
            let inner = tokio::net::UnixListener::bind(path)?;
            if let Err(error) =
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            {
                let _ = std::fs::remove_file(path);
                return Err(error);
            }
            Ok(Self { inner })
        }

        pub async fn accept(&self) -> io::Result<(LocalStream, tokio::net::unix::SocketAddr)> {
            self.inner.accept().await
        }
    }
}

#[cfg(unix)]
pub use unix_impl::{LocalListener, LocalStream};

#[cfg(all(test, unix))]
mod unix_tests {
    use std::os::unix::fs::PermissionsExt;

    use super::LocalListener;

    #[tokio::test]
    async fn socket_is_owner_only() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("local.sock");

        let listener = LocalListener::bind(&path).unwrap();

        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        drop(listener);
    }
}

/// Has a local process bound a listener at `path`?
///
/// - Unix: stats the socket file.
/// - Windows: probes the named pipe (Named Pipes don't appear in the
///   filesystem, so `path.exists()` doesn't work).
pub fn listener_is_ready(path: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        path.exists()
    }
    #[cfg(windows)]
    {
        windows_impl::listener_is_ready(path)
    }
}

#[cfg(windows)]
pub use windows_impl::{LocalListener, LocalStream};

#[cfg(windows)]
mod windows_impl {
    use std::ffi::OsStr;
    use std::io;
    use std::path::Path;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::time::Duration;

    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
    use tracing::debug;

    struct OwnerOnlySecurityAttributes {
        descriptor: windows::Win32::Security::PSECURITY_DESCRIPTOR,
        attributes: windows::Win32::Security::SECURITY_ATTRIBUTES,
    }

    impl OwnerOnlySecurityAttributes {
        fn new() -> io::Result<Self> {
            use windows::Win32::Security::Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            };
            use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
            use windows::core::PCWSTR;

            // A protected DACL that grants generic-all only to the object owner.
            // Named-pipe objects inherit the creating token's owner, i.e. the
            // current OS user, so other local users cannot open the endpoint.
            let sddl: Vec<u16> = "D:P(A;;GA;;;OW)\0".encode_utf16().collect();
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    PCWSTR(sddl.as_ptr()),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    None,
                )
                .map_err(|error| io::Error::new(io::ErrorKind::PermissionDenied, error))?;
            }

            Ok(Self {
                descriptor,
                attributes: SECURITY_ATTRIBUTES {
                    nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                    lpSecurityDescriptor: descriptor.0,
                    bInheritHandle: false.into(),
                },
            })
        }

        fn as_mut_ptr(&mut self) -> *mut std::ffi::c_void {
            (&mut self.attributes as *mut windows::Win32::Security::SECURITY_ATTRIBUTES).cast()
        }
    }

    impl Drop for OwnerOnlySecurityAttributes {
        fn drop(&mut self) {
            use windows::Win32::Foundation::{HLOCAL, LocalFree};

            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.descriptor.0)));
            }
        }
    }

    fn create_server(
        pipe_name: &OsStr,
        first_instance: bool,
    ) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
        use tokio::net::windows::named_pipe::ServerOptions;

        let mut options = ServerOptions::new();
        options
            .first_pipe_instance(first_instance)
            .reject_remote_clients(true);
        let mut security = OwnerOnlySecurityAttributes::new()?;
        unsafe { options.create_with_security_attributes_raw(pipe_name, security.as_mut_ptr()) }
    }

    /// Bidirectional IPC stream wrapping a connected named pipe (server-
    /// or client-side, depending on how it was created).
    pub struct LocalStream {
        inner: StreamInner,
    }

    enum StreamInner {
        Server(tokio::net::windows::named_pipe::NamedPipeServer),
        Client(tokio::net::windows::named_pipe::NamedPipeClient),
    }

    impl LocalStream {
        /// Connect to a listener at `path`. The path is translated to a
        /// named-pipe name and `ClientOptions::open` is used.
        pub async fn connect<P: AsRef<Path>>(path: P) -> io::Result<Self> {
            use tokio::net::windows::named_pipe::ClientOptions;

            // ClientOptions::open returns ERROR_PIPE_BUSY if all pipe
            // instances are in use; the caller's CONNECT_TIMEOUT loop
            // already retries, so we surface the error and let it handle.
            let pipe_name = path_to_pipe_name(path.as_ref());
            let inner = ClientOptions::new().open(pipe_name)?;
            Ok(Self {
                inner: StreamInner::Client(inner),
            })
        }
    }

    // tokio's NamedPipeServer / NamedPipeClient are auto-Unpin (they wrap
    // PollEvented<mio::windows::NamedPipe>, which is Unpin), so our
    // wrapping enum and struct are auto-Unpin as well. That means
    // Pin<&mut Self>::get_mut() is safe — no unsafe needed for the
    // structural projection into `inner`.
    impl AsyncRead for LocalStream {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            match &mut self.get_mut().inner {
                StreamInner::Server(s) => Pin::new(s).poll_read(cx, buf),
                StreamInner::Client(c) => Pin::new(c).poll_read(cx, buf),
            }
        }
    }

    impl AsyncWrite for LocalStream {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            match &mut self.get_mut().inner {
                StreamInner::Server(s) => Pin::new(s).poll_write(cx, buf),
                StreamInner::Client(c) => Pin::new(c).poll_write(cx, buf),
            }
        }

        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            match &mut self.get_mut().inner {
                StreamInner::Server(s) => Pin::new(s).poll_flush(cx),
                StreamInner::Client(c) => Pin::new(c).poll_flush(cx),
            }
        }

        fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            match &mut self.get_mut().inner {
                StreamInner::Server(s) => Pin::new(s).poll_shutdown(cx),
                StreamInner::Client(c) => Pin::new(c).poll_shutdown(cx),
            }
        }
    }

    /// Listener for incoming local IPC connections. Holds the pipe name
    /// plus the next pre-created server instance (Windows named pipes
    /// require pre-creating an instance per pending connection).
    pub struct LocalListener {
        pipe_name: std::ffi::OsString,
        /// Next pre-created server instance, ready for `connect().await`.
        /// We rotate: take this one, await its connect, immediately create
        /// the next one for the following accept(). The first instance is
        /// created in `bind()` with `first_pipe_instance(true)` to lock
        /// out other processes from squatting the pipe name.
        ///
        /// tokio::sync::Mutex (not parking_lot) because accept() holds the
        /// lock across `server.connect().await`.
        next_server: tokio::sync::Mutex<Option<tokio::net::windows::named_pipe::NamedPipeServer>>,
    }

    impl LocalListener {
        /// Reserve a named-pipe name (no on-disk file is created).
        pub fn bind<P: AsRef<Path>>(path: P) -> io::Result<Self> {
            let pipe_name = path_to_pipe_name(path.as_ref());
            let first = create_server(&pipe_name, true)?;
            Ok(Self {
                pipe_name,
                next_server: tokio::sync::Mutex::new(Some(first)),
            })
        }

        /// Wait for the next incoming connection. Mirrors
        /// `UnixListener::accept`, returning a connected stream and a unit
        /// placeholder where Unix would return the peer address (named
        /// pipes don't carry one).
        pub async fn accept(&self) -> io::Result<(LocalStream, ())> {
            // Take the pending instance (or create one), await a client, then
            // pre-create the next. On connect() error, drop the instance and
            // retry with a fresh one — returning early would leave the slot
            // empty and brick the listener. Bounded with a backoff so a
            // persistently failing connect() can't busy-spin.
            const MAX_ACCEPT_ATTEMPTS: usize = 10;
            const RETRY_BACKOFF: Duration = Duration::from_millis(20);

            let mut slot = self.next_server.lock().await;
            let mut last_err: Option<io::Error> = None;
            for attempt in 0..MAX_ACCEPT_ATTEMPTS {
                let server = match slot.take() {
                    Some(server) => server,
                    None => create_server(&self.pipe_name, false)?,
                };
                match server.connect().await {
                    Ok(()) => {
                        *slot = Some(create_server(&self.pipe_name, false)?);
                        return Ok((
                            LocalStream {
                                inner: StreamInner::Server(server),
                            },
                            (),
                        ));
                    }
                    Err(e) => {
                        // Failed `server` drops here, freeing the instance.
                        debug!(attempt, error = %e, "named-pipe accept connect failed; retrying");
                        last_err = Some(e);
                        tokio::time::sleep(RETRY_BACKOFF).await;
                    }
                }
            }

            // Best-effort re-arm; take-or-create above still recovers if this fails.
            if let Ok(fresh) = create_server(&self.pipe_name, false) {
                *slot = Some(fresh);
            }
            Err(last_err
                .unwrap_or_else(|| io::Error::other("LocalListener: accept exhausted retries")))
        }
    }

    /// Whether a local process has a pipe bound at `path`.
    ///
    /// Probes with `WaitNamedPipeW` (non-connecting), not `ClientOptions::open`,
    /// which would open a real client the leader's `accept()` consumes as a
    /// phantom session. `ERROR_FILE_NOT_FOUND` means absent; `TRUE` or any other
    /// error (e.g. `ERROR_SEM_TIMEOUT`: exists but busy) means ready.
    pub fn listener_is_ready(path: &Path) -> bool {
        use std::os::windows::ffi::OsStrExt;

        use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, GetLastError};
        use windows::Win32::System::Pipes::WaitNamedPipeW;
        use windows::core::PCWSTR;

        // 1 ms (a real timeout, not 0 = "server default").
        const PROBE_TIMEOUT_MS: u32 = 1;

        let pipe_name = path_to_pipe_name(path);
        let wide: Vec<u16> = pipe_name
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        if unsafe { WaitNamedPipeW(PCWSTR(wide.as_ptr()), PROBE_TIMEOUT_MS) }.as_bool() {
            return true;
        }
        // FALSE: only a missing pipe means not-ready.
        let err = unsafe { GetLastError() };
        err != ERROR_FILE_NOT_FOUND
    }

    /// Full named-pipe path: `\\.\pipe\<leaf>`.
    fn path_to_pipe_name(path: &Path) -> std::ffi::OsString {
        let mut name = std::ffi::OsString::from(r"\\.\pipe\");
        name.push(pipe_leaf_name(path));
        name
    }

    /// Deterministic leaf name (`grow-local-<hash>`) for a filesystem path.
    ///
    /// Uses SipHash-1-3 with fixed keys so the hash is stable across Rust
    /// versions (unlike `DefaultHasher`, whose algorithm is unspecified).
    fn pipe_leaf_name(path: &Path) -> std::ffi::OsString {
        use siphasher::sip::SipHasher13;
        use std::hash::{Hash, Hasher};

        // Fixed keys — must never change once shipped.
        let mut hasher = SipHasher13::new_with_keys(0x67726f6b_6c656164, 0x65725f70_69706521);
        path.hash(&mut hasher);
        let hash = hasher.finish();
        std::ffi::OsString::from(format!("grow-local-{hash:016x}"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::path::Path;

        #[test]
        fn pipe_name_is_deterministic() {
            let a = path_to_pipe_name(Path::new("/tmp/grow.sock"));
            let b = path_to_pipe_name(Path::new("/tmp/grow.sock"));
            assert_eq!(a, b);
        }

        #[test]
        fn different_paths_produce_different_names() {
            let a = path_to_pipe_name(Path::new("/tmp/a.sock"));
            let b = path_to_pipe_name(Path::new("/tmp/b.sock"));
            assert_ne!(a, b);
        }

        #[test]
        fn pipe_name_has_correct_prefix() {
            let name = path_to_pipe_name(Path::new("/tmp/test.sock"));
            let s = name.to_string_lossy();
            assert!(s.starts_with(r"\\.\pipe\grow-local-"), "got: {s}");
        }

        #[test]
        fn pipe_name_is_bounded() {
            let long_path = format!("/{}", "a".repeat(500));
            let name = path_to_pipe_name(Path::new(&long_path));
            // \\.\pipe\grow-local- (20 chars) + 16 hex chars = 36 total
            assert!(name.len() <= 256, "pipe name too long: {}", name.len());
        }

        #[tokio::test]
        async fn listener_is_ready_tracks_pipe_lifecycle() {
            // Unique path per process so parallel test binaries don't collide on
            // the derived pipe name.
            let path =
                std::env::temp_dir().join(format!("grow-ready-probe-{}.sock", std::process::id()));

            // Nothing bound yet -> ERROR_FILE_NOT_FOUND -> not ready.
            assert!(!listener_is_ready(&path));

            let listener = LocalListener::bind(&path).unwrap();
            // Ready as soon as the pipe is bound, before any accept().
            assert!(listener_is_ready(&path));

            // After the last instance is dropped the pipe name disappears.
            drop(listener);
            assert!(!listener_is_ready(&path));
        }
    }
}
