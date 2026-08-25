//! Background persistence for the tool runtime's registered Resources.
//!
//! [`ResourcesPersistence`] is the sole persistence path for this independent
//! state domain. Empty session paths construct a no-op handle explicitly.

use std::io;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::types::resources::Resources;

const MAX_RESOURCES_STATE_BYTES: u64 = 16 * 1024 * 1024;

/// Identity-bound storage capability for the tool runtime's independent state
/// domain. Implementations must never reopen `display_path` as authority.
pub trait ResourcesStateStore: Send + Sync {
    fn display_path(&self) -> &Path;
    fn read(&self) -> io::Result<Option<Vec<u8>>>;
    fn write_atomic(&self, bytes: &[u8], durable: bool) -> io::Result<()>;
}

/// Local capability used by non-session embedders and tests. The parent
/// directory is opened once; every later operation is relative to that pinned
/// handle and rejects a symlink/special-file target.
pub struct LocalResourcesStateStore {
    display_path: PathBuf,
    directory: cap_std::fs::Dir,
    name: std::ffi::OsString,
}

impl LocalResourcesStateStore {
    pub fn open(state_path: PathBuf) -> io::Result<Self> {
        let parent = state_path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "resources state has no parent")
        })?;
        let name = state_path
            .file_name()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "resources state has no file name",
                )
            })?
            .to_os_string();
        let directory = cap_std::fs::Dir::open_ambient_dir(parent, cap_std::ambient_authority())?;
        Ok(Self {
            display_path: state_path,
            directory,
            name,
        })
    }

    fn open_read(&self) -> io::Result<cap_std::fs::File> {
        use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt as _};

        let mut options = cap_std::fs::OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let file = self.directory.open_with(&self.name, &options)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "resources state is not a regular file",
            ));
        }
        Ok(file)
    }
}

impl ResourcesStateStore for LocalResourcesStateStore {
    fn display_path(&self) -> &Path {
        &self.display_path
    }

    fn read(&self) -> io::Result<Option<Vec<u8>>> {
        let file = match self.open_read() {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        if file.metadata()?.len() > MAX_RESOURCES_STATE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "resources state exceeds the byte limit",
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_RESOURCES_STATE_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_RESOURCES_STATE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "resources state grew while reading",
            ));
        }
        Ok(Some(bytes))
    }

    fn write_atomic(&self, bytes: &[u8], durable: bool) -> io::Result<()> {
        use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt as _};

        if bytes.len() as u64 > MAX_RESOURCES_STATE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "resources state exceeds the byte limit",
            ));
        }
        match self.directory.symlink_metadata(&self.name) {
            Ok(metadata) if !metadata.is_file() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "resources state target is not a regular file",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let tmp_name = format!(
            ".resources_state.{}.{}.tmp",
            std::process::id(),
            uuid::Uuid::now_v7().simple()
        );
        let mut options = cap_std::fs::OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        let mut file = self.directory.open_with(&tmp_name, &options)?;
        let result = (|| {
            file.write_all(bytes)?;
            if durable {
                file.sync_all()?;
            }
            drop(file);
            self.directory
                .rename(&tmp_name, &self.directory, &self.name)?;
            if durable {
                self.directory
                    .try_clone()?
                    .into_std_file()
                    .sync_all()
                    .map_err(published_persistence_error)?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = self.directory.remove_file(&tmp_name);
        }
        result
    }
}

#[derive(Debug)]
struct PublishedPersistenceError(io::Error);

impl std::fmt::Display for PublishedPersistenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (state was published; durability is uncertain)",
            self.0
        )
    }
}

impl std::error::Error for PublishedPersistenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

fn published_persistence_error(error: io::Error) -> io::Error {
    io::Error::new(error.kind(), PublishedPersistenceError(error))
}

/// Background persistence for `Resources` state/params.
///
/// Debounced background writes with atomic rename. Takes a `serde_json::Value`
/// from `Resources::serialize()` and writes it to disk. On load, parses the
/// JSON and feeds it to `Resources::load_from()`.
pub struct ResourcesPersistence {
    /// Display-only path for diagnostics.
    state_path: PathBuf,
    store: Option<Arc<dyn ResourcesStateStore>>,
    /// Channel to send serialized state to the background writer
    tx: tokio::sync::mpsc::UnboundedSender<ResourcesPersistenceCommand>,
    noop: bool,
}

#[cfg(test)]
pub(crate) type ControlledSave = (
    serde_json::Value,
    tokio::sync::oneshot::Sender<io::Result<()>>,
);

enum ResourcesPersistenceCommand {
    /// Write this serialized Resources value to disk
    Save(serde_json::Value),
    SaveAndFlush {
        snapshot: serde_json::Value,
        respond_to: tokio::sync::oneshot::Sender<io::Result<()>>,
    },
    /// Flush pending writes and notify when done
    Flush(tokio::sync::oneshot::Sender<()>),
}

impl ResourcesPersistence {
    /// Whether a durable write error happened after the target file was
    /// atomically published. Callers must not roll back matching in-memory
    /// state in this case: the visible file already contains the new value,
    /// even though the parent-directory fsync could not prove crash durability.
    pub fn error_was_published(error: &io::Error) -> bool {
        error
            .get_ref()
            .and_then(|source| source.downcast_ref::<PublishedPersistenceError>())
            .is_some()
    }

    /// Construct a noop persistence handle for tests. No background task.
    pub fn noop() -> Self {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            state_path: PathBuf::from("/dev/null"),
            store: None,
            tx,
            noop: true,
        }
    }

    #[cfg(test)]
    pub(crate) fn controlled() -> (Self, tokio::sync::mpsc::UnboundedReceiver<ControlledSave>) {
        let (tx, mut commands) =
            tokio::sync::mpsc::unbounded_channel::<ResourcesPersistenceCommand>();
        let (observed_tx, observed_rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(command) = commands.recv().await {
                match command {
                    ResourcesPersistenceCommand::Save(_) => {}
                    ResourcesPersistenceCommand::SaveAndFlush {
                        snapshot,
                        respond_to,
                    } => {
                        let _ = observed_tx.send((snapshot, respond_to));
                    }
                    ResourcesPersistenceCommand::Flush(done) => {
                        let _ = done.send(());
                    }
                }
            }
        });
        (
            Self {
                state_path: PathBuf::from("/dev/null"),
                store: None,
                tx,
                noop: false,
            },
            observed_rx,
        )
    }

    /// Create a persistence handle around an already established storage
    /// capability and spawn its single background writer.
    pub fn new(store: Arc<dyn ResourcesStateStore>) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let state_path = store.display_path().to_path_buf();
        let writer_store = store.clone();

        tokio::spawn(async move {
            Self::writer_loop(rx, writer_store).await;
        });

        Self {
            state_path,
            store: Some(store),
            tx,
            noop: false,
        }
    }

    /// Open a pinned local-file capability. Session runtimes should instead
    /// pass their existing session-directory capability to [`Self::new`].
    pub fn local(state_path: PathBuf) -> io::Result<Self> {
        Ok(Self::new(Arc::new(LocalResourcesStateStore::open(
            state_path,
        )?)))
    }

    /// Load existing Resources state from disk, if the file exists.
    ///
    /// Reads the JSON, parses it into the nested `HashMap<String, HashMap<String, Value>>`
    /// shape that `Resources::load_from()` expects, and applies it to the given resources.
    ///
    /// Returns `true` if state was loaded, `false` if no file or parse error.
    pub fn load(&self, resources: &mut Resources) -> bool {
        let Some(store) = &self.store else {
            return false;
        };
        let json = match store.read() {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return false,
            Err(error) => {
                tracing::warn!(?error, path = ?self.state_path, "Failed to read resources state");
                return false;
            }
        };

        let top: serde_json::Value = match serde_json::from_slice(&json) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    "Failed to parse resources state from {:?}: {}",
                    self.state_path,
                    e
                );
                return false;
            }
        };

        let data = match Self::value_to_nested_map(top) {
            Some(m) => m,
            None => {
                tracing::warn!(
                    "Resources state file {:?} has unexpected shape",
                    self.state_path
                );
                return false;
            }
        };

        resources.load_from(data);
        true
    }

    /// Save the current Resources state (non-blocking).
    /// Sends a serialized snapshot to the background writer.
    pub fn save(&self, resources: &Resources) {
        if self.noop {
            return;
        }
        let snapshot = resources.serialize();
        let _ = self.tx.send(ResourcesPersistenceCommand::Save(snapshot));
    }

    /// Replace pending snapshots, write this snapshot, and acknowledge the result.
    pub fn enqueue_save_and_flush(
        &self,
        snapshot: serde_json::Value,
    ) -> io::Result<tokio::sync::oneshot::Receiver<io::Result<()>>> {
        if self.noop {
            let (respond_to, response) = tokio::sync::oneshot::channel();
            let _ = respond_to.send(Ok(()));
            return Ok(response);
        }
        let (respond_to, response) = tokio::sync::oneshot::channel();
        self.tx
            .send(ResourcesPersistenceCommand::SaveAndFlush {
                snapshot,
                respond_to,
            })
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "resources persistence writer stopped",
                )
            })?;
        Ok(response)
    }

    /// Await an acknowledgement returned by [`Self::enqueue_save_and_flush`].
    pub async fn await_save_and_flush(
        response: tokio::sync::oneshot::Receiver<io::Result<()>>,
    ) -> io::Result<()> {
        response.await.map_err(|_| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "resources persistence writer dropped acknowledgement",
            )
        })?
    }

    /// Replace pending snapshots, write this snapshot, and await the result.
    pub async fn save_and_flush(&self, snapshot: serde_json::Value) -> io::Result<()> {
        Self::await_save_and_flush(self.enqueue_save_and_flush(snapshot)?).await
    }

    /// Path to the persisted state file.
    pub fn state_path(&self) -> &std::path::Path {
        &self.state_path
    }

    /// Flush pending writes. Call on graceful shutdown.
    pub async fn flush(&self) {
        if self.noop {
            return;
        }
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let _ = self.tx.send(ResourcesPersistenceCommand::Flush(done_tx));
        let _ = done_rx.await;
    }

    /// Convert the `serde_json::Value` (from serialize()) into the nested
    /// HashMap structure that `load_from()` expects.
    fn value_to_nested_map(
        val: serde_json::Value,
    ) -> Option<
        std::collections::HashMap<String, std::collections::HashMap<String, serde_json::Value>>,
    > {
        let top = val.as_object()?;
        let mut result = std::collections::HashMap::new();
        for (cat_key, cat_val) in top {
            let inner_obj = cat_val.as_object()?;
            let mut inner = std::collections::HashMap::new();
            for (k, v) in inner_obj {
                inner.insert(k.clone(), v.clone());
            }
            result.insert(cat_key.clone(), inner);
        }
        Some(result)
    }

    async fn writer_loop(
        mut rx: tokio::sync::mpsc::UnboundedReceiver<ResourcesPersistenceCommand>,
        store: Arc<dyn ResourcesStateStore>,
    ) {
        let mut pending: Option<serde_json::Value> = None;
        let mut debounce = tokio::time::interval(Duration::from_millis(500));
        debounce.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                cmd = rx.recv() => {
                    match cmd {
                        Some(ResourcesPersistenceCommand::Save(snapshot)) => {
                            pending = Some(snapshot);
                        }
                        Some(ResourcesPersistenceCommand::SaveAndFlush {
                            snapshot,
                            respond_to,
                        }) => {
                            pending = None;
                            let result = Self::write_json(store.clone(), &snapshot, true).await;
                            let _ = respond_to.send(result);
                        }
                        Some(ResourcesPersistenceCommand::Flush(done)) => {
                            if let Some(snapshot) = pending.take()
                                && let Err(error) = Self::write_json(store.clone(), &snapshot, false).await
                            {
                                tracing::warn!(
                                    ?error,
                                    path = ?store.display_path(),
                                    "Failed to flush resources state"
                                );
                            }
                            let _ = done.send(());
                        }
                        None => {
                            if let Some(snapshot) = pending.take()
                                && let Err(error) = Self::write_json(store.clone(), &snapshot, false).await
                            {
                                tracing::warn!(
                                    ?error,
                                    path = ?store.display_path(),
                                    "Failed to flush resources state"
                                );
                            }
                            break;
                        }
                    }
                }
                _ = debounce.tick() => {
                    if let Some(snapshot) = pending.take()
                        && let Err(error) = Self::write_json(store.clone(), &snapshot, false).await
                    {
                        tracing::warn!(
                            ?error,
                            path = ?store.display_path(),
                            "Failed to save resources state"
                        );
                    }
                }
            }
        }
    }

    async fn write_json(
        store: Arc<dyn ResourcesStateStore>,
        value: &serde_json::Value,
        durable: bool,
    ) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(value)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        tokio::task::spawn_blocking(move || store.write_atomic(&json, durable))
            .await
            .map_err(|error| io::Error::other(format!("resources writer task failed: {error}")))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ResourcesPersistence tests
    // -----------------------------------------------------------------------

    use crate::types::resources::{
        ModelImageInputKey, ModelImageInputState, Resources, State, WebCitationCounter,
    };

    #[tokio::test]
    async fn resources_save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("resources_state.json");

        let persistence = ResourcesPersistence::local(state_path).unwrap();

        // Build resources with registered state types
        let mut resources = Resources::new();
        resources.register_state::<WebCitationCounter>();

        // Populate WebCitationCounter
        {
            let counter = resources.get_or_default::<State<WebCitationCounter>>();
            counter.counter = 7;
        }

        // Save and flush
        persistence.save(&resources);
        persistence.flush().await;

        // Load into fresh resources (with same registrations)
        let mut restored = Resources::new();
        restored.register_state::<WebCitationCounter>();
        assert!(persistence.load(&mut restored));

        // Verify WebCitationCounter roundtripped
        let counter = restored.get::<State<WebCitationCounter>>().unwrap();
        assert_eq!(counter.counter, 7);
    }

    #[tokio::test]
    async fn model_image_input_state_roundtrips_per_runtime_identity() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("resources_state.json");
        let persistence = ResourcesPersistence::local(state_path).unwrap();
        let rejected = ModelImageInputKey::new("text-model", "messages", "endpoint-a");

        let mut resources = Resources::new();
        resources.register_state::<ModelImageInputState>();
        resources
            .get_or_default::<State<ModelImageInputState>>()
            .mark_unsupported(rejected.clone());
        persistence
            .save_and_flush(resources.serialize())
            .await
            .unwrap();

        let mut restored = Resources::new();
        restored.register_state::<ModelImageInputState>();
        assert!(persistence.load(&mut restored));
        let state = restored.get::<State<ModelImageInputState>>().unwrap();
        assert!(state.is_unsupported(&rejected));
        assert!(!state.is_unsupported(&ModelImageInputKey::new(
            "vision-model",
            "messages",
            "endpoint-a",
        )));
        assert!(!state.is_unsupported(&ModelImageInputKey::new(
            "text-model",
            "responses",
            "endpoint-a",
        )));
        assert!(!state.is_unsupported(&ModelImageInputKey::new(
            "text-model",
            "messages",
            "endpoint-b",
        )));
    }

    #[tokio::test]
    async fn resources_load_returns_false_when_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("nonexistent.json");

        let persistence = ResourcesPersistence::local(state_path).unwrap();
        let mut resources = Resources::new();
        assert!(!persistence.load(&mut resources));
    }

    #[tokio::test]
    async fn resources_load_returns_false_on_corrupt_json() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("resources_state.json");
        std::fs::write(&state_path, "{ this is not valid json }").unwrap();

        let persistence = ResourcesPersistence::local(state_path).unwrap();
        let mut resources = Resources::new();
        assert!(!persistence.load(&mut resources));
    }

    /// Atomic-rename guarantee: a concurrent reader hammering the path while
    /// the writer streams 200 snapshots must never observe torn JSON.
    #[tokio::test]
    async fn writer_atomic_rename_never_exposes_partial_json() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("resources_state.json");
        let persistence = ResourcesPersistence::local(state_path.clone()).unwrap();

        let mut resources = Resources::new();
        resources.register_state::<WebCitationCounter>();

        let done = std::sync::Arc::new(AtomicBool::new(false));
        let reader_done = done.clone();
        let reader_path = state_path.clone();
        let reader = tokio::spawn(async move {
            while !reader_done.load(Ordering::Relaxed) {
                if let Ok(s) = tokio::fs::read_to_string(&reader_path).await {
                    assert!(
                        serde_json::from_str::<serde_json::Value>(&s).is_ok(),
                        "reader observed a torn/partial write (atomic-rename violated): {s:?}"
                    );
                }
                tokio::task::yield_now().await;
            }
        });

        for i in 0..200u64 {
            {
                let counter = resources.get_or_default::<State<WebCitationCounter>>();
                counter.counter = i as u32;
            }
            persistence.save(&resources);
            persistence.flush().await;
        }

        done.store(true, Ordering::Relaxed);
        reader.await.unwrap();

        // Final state is intact and reflects the last write.
        let content = std::fs::read_to_string(&state_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(parsed["state"]["grow_build.WebCitation"].is_object());
    }

    #[tokio::test]
    async fn resources_flush_writes_pending() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("resources_state.json");

        let persistence = ResourcesPersistence::local(state_path.clone()).unwrap();

        let mut resources = Resources::new();
        resources.register_state::<WebCitationCounter>();
        {
            let counter = resources.get_or_default::<State<WebCitationCounter>>();
            counter.counter = 42;
        }

        persistence.save(&resources);
        persistence.flush().await;

        // File should exist with correct structure
        assert!(state_path.exists());
        let content = std::fs::read_to_string(&state_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        // Should have "state" category with "grow_build.WebCitation" key
        assert!(parsed["state"]["grow_build.WebCitation"].is_object());
    }

    #[tokio::test]
    async fn save_and_flush_supersedes_older_pending_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("resources_state.json");
        let persistence = ResourcesPersistence::local(state_path.clone()).unwrap();
        persistence.flush().await;

        let mut resources = Resources::new();
        resources.register_state::<WebCitationCounter>();
        resources
            .get_or_default::<State<WebCitationCounter>>()
            .counter = 1;
        persistence.save(&resources);

        resources
            .get_or_default::<State<WebCitationCounter>>()
            .counter = 2;
        persistence
            .save_and_flush(resources.serialize())
            .await
            .unwrap();
        persistence.flush().await;

        let content = std::fs::read_to_string(state_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["state"]["grow_build.WebCitation"]["counter"], 2);
    }

    #[tokio::test]
    async fn save_and_flush_error_can_be_retried() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("resources_state.json");
        std::fs::create_dir(&state_path).unwrap();
        let persistence = ResourcesPersistence::local(state_path.clone()).unwrap();

        let mut resources = Resources::new();
        resources.register_state::<WebCitationCounter>();
        resources
            .get_or_default::<State<WebCitationCounter>>()
            .counter = 7;
        let snapshot = resources.serialize();

        assert!(persistence.save_and_flush(snapshot.clone()).await.is_err());

        std::fs::remove_dir(&state_path).unwrap();
        persistence.save_and_flush(snapshot).await.unwrap();

        let content = std::fs::read_to_string(state_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["state"]["grow_build.WebCitation"]["counter"], 7);
    }

    #[tokio::test]
    async fn enqueued_acknowledged_save_precedes_a_newer_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("resources_state.json");
        let persistence = ResourcesPersistence::local(state_path.clone()).unwrap();
        persistence.flush().await;

        let mut resources = Resources::new();
        resources.register_state::<WebCitationCounter>();
        resources
            .get_or_default::<State<WebCitationCounter>>()
            .counter = 1;
        let acknowledgement = persistence
            .enqueue_save_and_flush(resources.serialize())
            .unwrap();

        resources
            .get_or_default::<State<WebCitationCounter>>()
            .counter = 2;
        persistence.save(&resources);

        ResourcesPersistence::await_save_and_flush(acknowledgement)
            .await
            .unwrap();
        persistence.flush().await;

        let content = std::fs::read_to_string(state_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["state"]["grow_build.WebCitation"]["counter"], 2);
    }

    #[tokio::test]
    async fn failed_publish_cleans_generated_temp() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("resources_state.json");
        std::fs::create_dir(&state_path).unwrap();
        let persistence = ResourcesPersistence::local(state_path).unwrap();
        assert!(
            persistence
                .save_and_flush(serde_json::json!({"state": {}}))
                .await
                .is_err()
        );
        assert!(std::fs::read_dir(dir.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));
    }

    #[test]
    fn published_failure_is_distinguishable_from_pre_publish_failure() {
        let before = io::Error::other("rename failed");
        assert!(!ResourcesPersistence::error_was_published(&before));

        let after = published_persistence_error(io::Error::other("directory fsync failed"));
        assert!(ResourcesPersistence::error_was_published(&after));
        assert!(after.to_string().contains("state was published"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn pinned_parent_resists_path_replacement_and_rejects_target_symlink() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("session");
        let moved = dir.path().join("moved");
        let external = dir.path().join("external");
        std::fs::create_dir(&session).unwrap();
        std::fs::create_dir(&external).unwrap();
        let state_path = session.join("resources_state.json");
        let persistence = ResourcesPersistence::local(state_path).unwrap();
        std::fs::rename(&session, &moved).unwrap();
        symlink(&external, &session).unwrap();
        persistence
            .save_and_flush(serde_json::json!({"state": {}}))
            .await
            .unwrap();
        assert!(moved.join("resources_state.json").is_file());
        assert!(!external.join("resources_state.json").exists());

        std::fs::remove_file(moved.join("resources_state.json")).unwrap();
        symlink(external.join("secret"), moved.join("resources_state.json")).unwrap();
        assert!(
            persistence
                .save_and_flush(serde_json::json!({"state": {}}))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn noop_save_and_flush_acknowledges_without_writing() {
        ResourcesPersistence::noop()
            .save_and_flush(serde_json::json!({"state": {}}))
            .await
            .unwrap();
    }
}
