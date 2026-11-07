//! Read-only filesystem helpers backing the client-facing
//! `workspace.client_fs_*` RPCs (the service.example.com conversation-files UI,
//! tunneled through the server).
//!
//! Deliberately separate from the shell-facing ext ops in
//! [`ext_fs`](super::ext_fs): every path here is workspace-root-relative
//! and resolves through the root-confinement helper
//! (`WorkspaceHandle::resolve_service_path`), the list walk excludes
//! symlinks that resolve outside the root (and never descends into
//! them), listings paginate with stable post-sort slices, and reads are
//! binary-safe (base64 chunks).
//!
//! Wire types live in `workspace_types::rpc::fs` (the
//! `ClientFs*` types), shared with the backend caller.

use std::path::{Path, PathBuf};

use workspace_types::rpc::fs::{
    ClientFsListNode as FsListNode, ClientFsListReq as FsListReq, ClientFsListRes as FsListRes,
    ClientFsReadFileReq as FsReadFileReq, ClientFsReadFileRes as FsReadFileRes,
    ClientFsStatReq as FsStatReq, ClientFsStatRes as FsStatRes, FsContentType, FsNodeType,
};

use crate::error::{WorkspaceError, WorkspaceResult};
use crate::handle::WorkspaceHandle;

/// Hard cap on entries collected per list call before sorting (shared
/// across all fs surfaces; see [`super::walk::MAX_LIST_COLLECT`]).
const MAX_LIST_COLLECT: usize = super::walk::MAX_LIST_COLLECT;

/// Server-side cap on `FsListReq::limit`.
const MAX_LIST_LIMIT: u32 = 1000;

/// Server-side cap on a single read's effective byte budget (shared
/// across all fs surfaces; see [`super::walk::MAX_READ_BYTES`]). Only
/// referenced by tests now that the clamp lives in `walk::clamp_read_length`.
#[cfg(test)]
const MAX_READ_BYTES: u64 = super::walk::MAX_READ_BYTES;

// =========================================================================
// Path resolution
// =========================================================================

/// Resolve a root-relative request path through the workspace's
/// root-confinement helper, returning the resolved path together with the
/// canonical root it was checked against. `""` and `"."` mean the
/// workspace root; absolute paths, `..` escapes, and symlink escapes are
/// rejected there.
async fn resolve_with_root(
    ws: &WorkspaceHandle,
    path: &str,
) -> WorkspaceResult<(PathBuf, PathBuf)> {
    let rel = if path.is_empty() { "." } else { path };
    let canonical_root = ws.canonical_root().await?;
    let abs = ws.resolve_service_path(rel, &canonical_root).await?;
    Ok((abs, canonical_root))
}

/// [`resolve_with_root`] for callers that don't need the canonical root.
async fn resolve(ws: &WorkspaceHandle, path: &str) -> WorkspaceResult<PathBuf> {
    resolve_with_root(ws, path).await.map(|(abs, _)| abs)
}

fn system_time_ms(st: std::time::SystemTime) -> i64 {
    match st.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_millis()).unwrap_or(i64::MAX),
    }
}

// =========================================================================
// list
// =========================================================================

/// List `req.path` (workspace-root-relative) with stable pagination:
/// collect the full walk (bounded by [`MAX_LIST_COLLECT`]), sort
/// directories-first / case-insensitive by name, then slice
/// `[offset, offset + limit)`. Symlinks resolving outside the workspace
/// root are excluded from the walk (and never descended into).
pub(crate) async fn list(ws: &WorkspaceHandle, req: &FsListReq) -> WorkspaceResult<FsListRes> {
    let (abs, canonical_root) = resolve_with_root(ws, &req.path).await?;
    let root = ws.root_cwd()?;
    let req = req.clone();
    // The walk does synchronous traversal + metadata syscalls; run it off
    // the async executor (matching the ext_fs ops).
    tokio::task::spawn_blocking(move || {
        list_blocking(&abs, &root, &canonical_root, &req, MAX_LIST_COLLECT)
    })
    .await
    .map_err(|e| WorkspaceError::JoinError(e.to_string()))?
}

fn list_blocking(
    abs_dir: &Path,
    root: &Path,
    canonical_root: &Path,
    req: &FsListReq,
    max_collect: usize,
) -> WorkspaceResult<FsListRes> {
    // Root confinement also holds mid-walk: a symlink inside the root
    // pointing outside must not enumerate outside metadata.
    let page = super::walk::list_directory_paged(
        abs_dir,
        super::walk::ListOptions {
            depth: req.depth as usize,
            follow_symlinks: req.follow_symlinks,
            respect_git_ignore: req.respect_git_ignore,
            include_hidden: req.include_hidden,
            include_globs: &req.include_globs,
            exclude_globs: &req.exclude_globs,
            offset: req.offset,
            limit: req.limit.min(MAX_LIST_LIMIT) as usize,
            confine_to_canonical_root: Some(canonical_root.to_path_buf()),
        },
        max_collect,
    );

    let nodes: Vec<FsListNode> = page
        .entries
        .into_iter()
        .map(|e| FsListNode {
            node_type: if e.is_dir {
                FsNodeType::Directory
            } else {
                FsNodeType::File
            },
            size: e.size,
            mtime_ms: e.modified.map(system_time_ms),
            is_symlink: e.is_symlink.then_some(true),
            // Root-relative path (divergent from the shell's absolute path).
            // A walk under a symlinked root yields canonical-root-spelled
            // entries, so strip either spelling.
            path: e
                .abs_path
                .strip_prefix(root)
                .or_else(|_| e.abs_path.strip_prefix(canonical_root))
                .unwrap_or(&e.abs_path)
                .to_string_lossy()
                .into_owned(),
            name: e.name,
        })
        .collect();

    Ok(FsListRes {
        nodes,
        truncated: page.truncated,
    })
}

// =========================================================================
// stat
// =========================================================================

/// Stat `req.path`: existence, kind, mtime — and, for files, a
/// full-content SHA-256 plus size taken from one streamed read of the
/// current content. `(size, mtime_ms)` cannot prove content generation
/// (same-length rewrites that preserve mtime, atomic replacement), so no
/// metadata tuple ever substitutes for hashing what is actually on disk.
pub(crate) async fn stat(ws: &WorkspaceHandle, req: &FsStatReq) -> WorkspaceResult<FsStatRes> {
    let abs = resolve(ws, &req.path).await?;
    let md = match tokio::fs::metadata(&abs).await {
        Ok(md) => md,
        // NotADirectory: a *file* sits mid-path (e.g. `a.txt/sub`) — for an
        // existence probe that is a miss, not an RPC error.
        Err(e)
            if e.kind() == std::io::ErrorKind::NotFound
                || e.kind() == std::io::ErrorKind::NotADirectory =>
        {
            return Ok(FsStatRes {
                exists: false,
                node_type: None,
                size: None,
                mtime_ms: None,
                hash: None,
            });
        }
        Err(e) => {
            return Err(WorkspaceError::Operation(format!(
                "stat failed for {}: {e}",
                req.path
            )));
        }
    };
    let mtime_ms = md.modified().ok().map(system_time_ms);
    if md.is_dir() {
        return Ok(FsStatRes {
            exists: true,
            node_type: Some(FsNodeType::Directory),
            size: None,
            mtime_ms,
            hash: None,
        });
    }
    // Full-stream pass: the digest and the reported size describe the same
    // read of the currently-opened file, not pre-read metadata.
    let (hash, _, streamed) = crate::handle::stream_hash_and_range(&abs, 0, 0)
        .await
        .map_err(|e| WorkspaceError::Operation(format!("hash failed for {}: {e}", req.path)))?;
    Ok(FsStatRes {
        exists: true,
        node_type: Some(FsNodeType::File),
        size: Some(streamed),
        mtime_ms,
        hash: Some(hash),
    })
}

// =========================================================================
// read_file
// =========================================================================

/// Read a byte range of `req.path` (binary-safe, capped at
/// `min(req.max_bytes, MAX_READ_BYTES)`) together with the full-file
/// SHA-256. One streamed pass (via the shared
/// [`crate::handle::stream_hash_and_range`]) yields the hash, the range
/// bytes and the reported size from the same read of the
/// currently-opened file; metadata only decides the file-vs-directory
/// error branch.
pub(crate) async fn read_file(
    ws: &WorkspaceHandle,
    req: &FsReadFileReq,
) -> WorkspaceResult<FsReadFileRes> {
    let abs = resolve(ws, &req.path).await?;
    let read_err =
        |e: std::io::Error| WorkspaceError::Operation(format!("read failed for {}: {e}", req.path));
    let md = tokio::fs::metadata(&abs).await.map_err(read_err)?;
    if md.is_dir() {
        return Err(WorkspaceError::Operation(format!(
            "not a file: {}",
            req.path
        )));
    }
    let offset = req.offset.unwrap_or(0);
    // Server-side clamp: a hostile/buggy caller cannot lift the per-chunk
    // budget past MAX_READ_BYTES regardless of `maxBytes`.
    let length = super::walk::clamp_read_length(req.length, req.max_bytes);

    // Single streamed pass: digest, range bytes and size are consistent
    // with each other and with the content actually on disk.
    let (hash, chunk, size) = crate::handle::stream_hash_and_range(&abs, offset, length)
        .await
        .map_err(read_err)?;

    // Shared encoder keeps the paired wire fields coherent with one UTF-8
    // validation pass; `type` is text iff the bytes were valid UTF-8.
    let (payload, is_text) = super::walk::encode_chunk(chunk, req.encoding);
    let (content, content_base64) = match payload {
        super::walk::ChunkPayload::Text(t) => (Some(t), None),
        super::walk::ChunkPayload::Base64(b) => (None, Some(b)),
    };
    let content_type = if is_text {
        FsContentType::Text
    } else {
        FsContentType::Binary
    };
    Ok(FsReadFileRes {
        content,
        content_base64,
        size,
        hash,
        content_type,
    })
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use workspace_types::rpc::fs::FsReadEncoding;

    use super::*;
    use crate::handle::tests::make_handle;

    fn list_req(path: &str) -> FsListReq {
        FsListReq {
            path: path.to_owned(),
            depth: 1,
            include_hidden: true,
            limit: 1000,
            offset: 0,
            follow_symlinks: true,
            respect_git_ignore: false,
            include_globs: vec![],
            exclude_globs: vec![],
        }
    }

    /// Fixture: root with files `b.txt`, `A.txt`, `c.txt` and dirs
    /// `Zeta`, `alpha`. Expected order: dirs first case-insensitive
    /// (`alpha`, `Zeta`), then files (`A.txt`, `b.txt`, `c.txt`).
    fn populate(root: &Path) {
        std::fs::write(root.join("b.txt"), b"bb").unwrap();
        std::fs::write(root.join("A.txt"), b"a").unwrap();
        std::fs::write(root.join("c.txt"), b"ccc").unwrap();
        std::fs::create_dir(root.join("Zeta")).unwrap();
        std::fs::create_dir(root.join("alpha")).unwrap();
    }

    /// `list_blocking` against `dir` as both walk root and workspace root
    /// (canonicalized for the confinement check, like production).
    fn list_dir(dir: &Path, req: &FsListReq, max_collect: usize) -> FsListRes {
        let canonical = dunce::canonicalize(dir).unwrap();
        list_blocking(dir, dir, &canonical, req, max_collect).unwrap()
    }

    #[test]
    fn list_sorts_dirs_first_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        populate(dir.path());
        let res = list_dir(dir.path(), &list_req(""), MAX_LIST_COLLECT);
        let names: Vec<&str> = res.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["alpha", "Zeta", "A.txt", "b.txt", "c.txt"]);
        assert!(!res.truncated);
        assert_eq!(res.nodes[0].node_type, FsNodeType::Directory);
        assert_eq!(res.nodes[2].node_type, FsNodeType::File);
        assert_eq!(res.nodes[2].size, Some(1));
        assert!(res.nodes[2].mtime_ms.is_some());
        // Paths are workspace-root-relative.
        assert_eq!(res.nodes[2].path, "A.txt");
    }

    /// Pagination slices the *sorted* listing, so consecutive pages have
    /// stable boundaries and concatenate to the full listing.
    #[test]
    fn list_paginates_post_sort_with_stable_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        populate(dir.path());
        let full = list_dir(dir.path(), &list_req(""), MAX_LIST_COLLECT);

        let mut paged = Vec::new();
        for page_start in [0u64, 2, 4] {
            let req = FsListReq {
                limit: 2,
                offset: page_start,
                ..list_req("")
            };
            let page = list_dir(dir.path(), &req, MAX_LIST_COLLECT);
            // truncated while more entries remain past this page.
            assert_eq!(page.truncated, page_start + 2 < full.nodes.len() as u64);
            paged.extend(page.nodes);
        }
        assert_eq!(paged, full.nodes);

        // Offset past the end yields an empty, non-truncated page.
        let req = FsListReq {
            offset: 100,
            ..list_req("")
        };
        let page = list_dir(dir.path(), &req, MAX_LIST_COLLECT);
        assert!(page.nodes.is_empty());
        assert!(!page.truncated);
    }

    /// The collection cap marks the result truncated even when the page
    /// itself is not full.
    #[test]
    fn list_collection_cap_truncates() {
        let dir = tempfile::tempdir().unwrap();
        populate(dir.path());
        let res = list_dir(dir.path(), &list_req(""), 2);
        assert_eq!(res.nodes.len(), 2);
        assert!(res.truncated);
    }

    #[test]
    fn list_caps_limit_at_server_max() {
        let dir = tempfile::tempdir().unwrap();
        populate(dir.path());
        let req = FsListReq {
            limit: u32::MAX,
            ..list_req("")
        };
        // Must not panic / overflow; the page is everything (< 1000).
        let res = list_dir(dir.path(), &req, MAX_LIST_COLLECT);
        assert_eq!(res.nodes.len(), 5);
    }

    /// Regression: the list walk must not traverse — or
    /// even surface — in-root symlinks that resolve outside the workspace
    /// root, while symlinks staying inside the root keep working.
    #[test]
    #[cfg(unix)]
    fn list_excludes_symlink_escapes_mid_walk() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), b"secret").unwrap();
        // Escaping symlink: root/escape_link -> <outside>.
        std::os::unix::fs::symlink(outside.path(), root.join("escape_link")).unwrap();
        // In-root symlink: root/good_link -> root/real_dir.
        std::fs::create_dir(root.join("real_dir")).unwrap();
        std::fs::write(root.join("real_dir/inner.txt"), b"inner").unwrap();
        std::os::unix::fs::symlink(root.join("real_dir"), root.join("good_link")).unwrap();

        let req = FsListReq {
            depth: 2,
            follow_symlinks: true,
            ..list_req("")
        };
        let res = list_dir(root, &req, MAX_LIST_COLLECT);
        let paths: Vec<&str> = res.nodes.iter().map(|n| n.path.as_str()).collect();
        assert!(
            !paths.iter().any(|p| p.contains("escape_link")),
            "escaping symlink (and its subtree) must be excluded: {paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p.contains("secret.txt")),
            "outside entries must not be enumerated: {paths:?}"
        );
        // Confinement must not over-filter: in-root symlinks survive,
        // including descent through them.
        assert!(paths.contains(&"good_link"), "{paths:?}");
        assert!(paths.contains(&"good_link/inner.txt"), "{paths:?}");
        assert!(paths.contains(&"real_dir/inner.txt"), "{paths:?}");
        let good = res.nodes.iter().find(|n| n.path == "good_link").unwrap();
        assert_eq!(good.is_symlink, Some(true));
    }

    fn sha256_hex(data: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(data))
    }

    /// Restore `path`'s timestamps to `mtime` — used to keep a rewrite
    /// inside the `(size, mtime_ms)` pair the removed WS-02 memo trusted.
    fn set_mtime(path: &Path, mtime: std::time::SystemTime) {
        let f = std::fs::File::options().write(true).open(path).unwrap();
        f.set_times(
            std::fs::FileTimes::new()
                .set_accessed(mtime)
                .set_modified(mtime),
        )
        .unwrap();
    }

    /// Regression (WS-02): a same-length rewrite that restores the file's
    /// mtime leaves `(path, size, mtime_ms)` unchanged — the exact key the
    /// old hash memo trusted as "content unchanged", so it served the
    /// previous digest. The digest must describe the bytes on disk now.
    #[tokio::test]
    async fn stat_hash_follows_same_size_same_mtime_rewrite() {
        use crate::workspace_ops::WorkspaceOp;

        let ws = make_handle();
        let root = ws.root_cwd().unwrap();
        let abs = root.join("data.txt");
        let v1 = [b'x'; 32];
        let v2 = [b'y'; 32];
        std::fs::write(&abs, v1).unwrap();

        let req = FsStatReq {
            path: "data.txt".into(),
        };
        let first = req.execute(&ws, None).await.unwrap();
        assert!(first.exists);
        assert_eq!(first.node_type, Some(FsNodeType::File));
        assert_eq!(first.size, Some(32));
        assert_eq!(first.hash, Some(sha256_hex(&v1)));

        // Same-length rewrite with the original mtime restored.
        let mtime = std::fs::metadata(&abs).unwrap().modified().unwrap();
        std::fs::write(&abs, v2).unwrap();
        set_mtime(&abs, mtime);

        let second = req.execute(&ws, None).await.unwrap();
        // The mutation is invisible in the metadata the old memo keyed on…
        assert_eq!(second.mtime_ms, first.mtime_ms);
        // …yet digest and size must describe the new bytes.
        assert_eq!(second.hash, Some(sha256_hex(&v2)));
        assert_eq!(second.size, Some(32));
    }

    /// Regression (WS-02): memo hits previously served `read_file` from a
    /// second range-only read, pairing fresh bytes with the stale digest
    /// and size. A rewrite that keeps the length and stays inside the
    /// original mtime millisecond must still yield digest, range and size
    /// from one current read.
    #[tokio::test]
    async fn read_file_returns_current_bytes_after_same_ms_rewrite() {
        use crate::workspace_ops::WorkspaceOp;

        let ws = make_handle();
        let root = ws.root_cwd().unwrap();
        let abs = root.join("data.bin");
        let v1 = [0x11u8; 64];
        let v2 = [0x22u8; 64];
        // FsReadFileRes carries plain size/hash (only FsStatRes wraps
        // them in Option). A fixed base mtime — 100 ns past the epoch
        // second 1700000000 — keeps the +1 ns rewrite bump provably
        // inside the same millisecond, whatever the wall clock reads.
        let mtime = std::time::SystemTime::UNIX_EPOCH
            .checked_add(std::time::Duration::from_secs(1_700_000_000))
            .unwrap()
            .checked_add(std::time::Duration::from_nanos(100))
            .unwrap();
        std::fs::write(&abs, v1).unwrap();
        set_mtime(&abs, mtime);

        let req = FsReadFileReq {
            path: "data.bin".into(),
            offset: Some(16),
            length: Some(8),
            max_bytes: 1_048_576,
            encoding: FsReadEncoding::Base64,
        };
        let first = req.execute(&ws, None).await.unwrap();
        assert_eq!(first.size, 64);
        assert_eq!(first.hash, sha256_hex(&v1));
        let first_bytes = base64::engine::general_purpose::STANDARD
            .decode(first.content_base64.unwrap())
            .unwrap();
        assert_eq!(first_bytes, v1[16..24]);

        // Same-length rewrite; the restored mtime sits one nanosecond
        // later inside the original millisecond.
        let bumped = mtime
            .checked_add(std::time::Duration::from_nanos(1))
            .unwrap();
        std::fs::write(&abs, v2).unwrap();
        set_mtime(&abs, bumped);
        let now_ms = system_time_ms(std::fs::metadata(&abs).unwrap().modified().unwrap());
        assert_eq!(
            now_ms,
            system_time_ms(mtime),
            "mutation must stay inside the original mtime millisecond"
        );

        let second = req.execute(&ws, None).await.unwrap();
        assert_eq!(second.size, 64);
        assert_eq!(second.hash, sha256_hex(&v2));
        let second_bytes = base64::engine::general_purpose::STANDARD
            .decode(second.content_base64.unwrap())
            .unwrap();
        assert_eq!(second_bytes, v2[16..24]);
    }

    /// Regression (WS-02): atomic replacement via rename swaps the inode
    /// while equal length and a restored mtime keep the old memo key
    /// valid. Both stat and read must follow the replaced content.
    #[tokio::test]
    async fn stat_and_read_follow_atomic_replacement_with_preserved_mtime() {
        use crate::workspace_ops::WorkspaceOp;

        let ws = make_handle();
        let root = ws.root_cwd().unwrap();
        let abs = root.join("data.txt");
        let v1 = [b'o'; 32];
        let v2 = [b'n'; 32];
        std::fs::write(&abs, v1).unwrap();
        let mtime = std::fs::metadata(&abs).unwrap().modified().unwrap();

        let stat_req = FsStatReq {
            path: "data.txt".into(),
        };
        let read_req = FsReadFileReq {
            path: "data.txt".into(),
            offset: None,
            length: None,
            max_bytes: 1_048_576,
            encoding: FsReadEncoding::Utf8,
        };
        let before = stat_req.execute(&ws, None).await.unwrap();
        assert_eq!(before.hash, Some(sha256_hex(&v1)));

        // Atomic replacement from a sibling temp file, mtime restored.
        let tmp = root.join(".data.txt.repl");
        std::fs::write(&tmp, v2).unwrap();
        std::fs::rename(&tmp, &abs).unwrap();
        set_mtime(&abs, mtime);

        let stat = stat_req.execute(&ws, None).await.unwrap();
        assert_eq!(stat.mtime_ms, before.mtime_ms);
        assert_eq!(stat.hash, Some(sha256_hex(&v2)));
        assert_eq!(stat.size, Some(32));

        let read = read_req.execute(&ws, None).await.unwrap();
        assert_eq!(read.hash, sha256_hex(&v2));
        assert_eq!(read.size, 32);
        assert_eq!(
            read.content.as_deref(),
            Some(std::str::from_utf8(&v2).unwrap())
        );
    }

    /// A path with a *file* as an intermediate component (`ENOTDIR`) is an
    /// existence miss, not an RPC error.
    #[tokio::test]
    async fn stat_enotdir_intermediate_reports_not_exists() {
        let ws = make_handle();
        let root = ws.root_cwd().unwrap();
        std::fs::write(root.join("file.txt"), b"x").unwrap();
        let res = stat(
            &ws,
            &FsStatReq {
                path: "file.txt/nested".into(),
            },
        )
        .await
        .unwrap();
        assert!(!res.exists);
        assert_eq!(res.node_type, None);
        assert_eq!(res.hash, None);
    }

    #[tokio::test]
    async fn stat_missing_path_reports_not_exists() {
        let ws = make_handle();
        let res = stat(
            &ws,
            &FsStatReq {
                path: "nope.txt".into(),
            },
        )
        .await
        .unwrap();
        assert!(!res.exists);
        assert_eq!(res.node_type, None);
        assert_eq!(res.hash, None);
    }

    #[tokio::test]
    async fn read_file_chunks_are_binary_safe_and_capped() {
        let ws = make_handle();
        let root = ws.root_cwd().unwrap();
        // Non-UTF-8 payload: every byte value once.
        let payload: Vec<u8> = (0u8..=255).collect();
        std::fs::write(root.join("blob.bin"), &payload).unwrap();

        let req = FsReadFileReq {
            path: "blob.bin".into(),
            // Bytes 200..210 are bare continuation bytes — never valid UTF-8.
            offset: Some(200),
            length: Some(50),
            max_bytes: 10, // cap below the requested length
            encoding: FsReadEncoding::Base64,
        };
        let res = read_file(&ws, &req).await.unwrap();
        assert_eq!(res.size, 256);
        assert_eq!(res.content, None);
        assert_eq!(res.content_type, FsContentType::Binary);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(res.content_base64.unwrap())
            .unwrap();
        assert_eq!(bytes, payload[200..210], "maxBytes caps the chunk");

        // Full-file hash regardless of the requested range.
        use sha2::{Digest, Sha256};
        assert_eq!(res.hash, format!("{:x}", Sha256::digest(&payload)));

        // A second read re-streams the file and returns the identical
        // chunk + hash.
        let again = read_file(&ws, &req).await.unwrap();
        assert_eq!(again.hash, res.hash);
        let again_bytes = base64::engine::general_purpose::STANDARD
            .decode(again.content_base64.unwrap())
            .unwrap();
        assert_eq!(again_bytes, payload[200..210]);
    }

    /// `maxBytes` is server-capped at [`MAX_READ_BYTES`]:
    /// a caller-supplied huge budget cannot make the workspace buffer the
    /// whole file.
    #[tokio::test]
    async fn read_file_server_caps_max_bytes() {
        let ws = make_handle();
        let root = ws.root_cwd().unwrap();
        let payload = vec![0u8; (MAX_READ_BYTES + 100) as usize];
        std::fs::write(root.join("big.bin"), &payload).unwrap();

        let res = read_file(
            &ws,
            &FsReadFileReq {
                path: "big.bin".into(),
                offset: None,
                length: None,
                max_bytes: u64::MAX,
                encoding: FsReadEncoding::Base64,
            },
        )
        .await
        .unwrap();
        assert_eq!(res.size, payload.len() as u64);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(res.content_base64.unwrap())
            .unwrap();
        assert_eq!(
            bytes.len() as u64,
            MAX_READ_BYTES,
            "clamped to the server cap"
        );
    }

    #[tokio::test]
    async fn read_file_utf8_default_and_binary_fallback() {
        let ws = make_handle();
        let root = ws.root_cwd().unwrap();
        std::fs::write(root.join("text.txt"), "héllo").unwrap();
        std::fs::write(root.join("bin.dat"), [0xff, 0xfe, 0x00]).unwrap();

        let text = read_file(
            &ws,
            &FsReadFileReq {
                path: "text.txt".into(),
                offset: None,
                length: None,
                max_bytes: 1_048_576,
                encoding: FsReadEncoding::Utf8,
            },
        )
        .await
        .unwrap();
        assert_eq!(text.content.as_deref(), Some("héllo"));
        assert_eq!(text.content_base64, None);
        assert_eq!(text.content_type, FsContentType::Text);

        // Invalid UTF-8 under the utf8 default degrades to base64.
        let bin = read_file(
            &ws,
            &FsReadFileReq {
                path: "bin.dat".into(),
                offset: None,
                length: None,
                max_bytes: 1_048_576,
                encoding: FsReadEncoding::Utf8,
            },
        )
        .await
        .unwrap();
        assert_eq!(bin.content, None);
        assert_eq!(bin.content_type, FsContentType::Binary);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(bin.content_base64.unwrap())
            .unwrap();
        assert_eq!(bytes, [0xff, 0xfe, 0x00]);
    }

    #[tokio::test]
    async fn resolve_rejects_escapes() {
        let ws = make_handle();
        for path in ["/etc/passwd", "../escape.txt"] {
            let err = stat(
                &ws,
                &FsStatReq {
                    path: path.to_owned(),
                },
            )
            .await
            .expect_err("escape must be rejected");
            assert!(matches!(err, WorkspaceError::Operation(_)), "{err:?}");
        }
    }

    /// An absolute path *inside* the workspace root is accepted and stats
    /// the same file as its root-relative form.
    #[tokio::test]
    async fn resolve_accepts_absolute_within_root() {
        let ws = make_handle();
        let root = ws.root_cwd().unwrap();
        std::fs::write(root.join("data.txt"), b"hello").unwrap();

        let rel = stat(
            &ws,
            &FsStatReq {
                path: "data.txt".into(),
            },
        )
        .await
        .unwrap();
        assert!(rel.exists);

        let abs_path = root.join("data.txt").to_string_lossy().into_owned();
        let abs = stat(&ws, &FsStatReq { path: abs_path }).await.unwrap();
        assert!(abs.exists);
        assert_eq!(abs.node_type, rel.node_type);
        assert_eq!(abs.size, rel.size);
        assert_eq!(abs.hash, rel.hash);
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn resolve_rejects_symlink_escape() {
        let ws = make_handle();
        let root = ws.root_cwd().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), b"secret").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("escape_link")).unwrap();

        let err = read_file(
            &ws,
            &FsReadFileReq {
                path: "escape_link/secret.txt".into(),
                offset: None,
                length: None,
                max_bytes: 1_048_576,
                encoding: FsReadEncoding::Base64,
            },
        )
        .await
        .expect_err("symlink escape must be rejected");
        assert!(
            err.to_string().contains("symlink escape"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn list_empty_path_lists_root() {
        let ws = make_handle();
        let root = ws.root_cwd().unwrap();
        std::fs::write(root.join("rooted.txt"), b"x").unwrap();
        let res = list(&ws, &list_req("")).await.unwrap();
        assert!(res.nodes.iter().any(|n| n.name == "rooted.txt"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn client_list_preserves_entry_and_symlink_metadata_through_workspace_operation() {
        use crate::workspace_ops::WorkspaceOp;

        let ws = make_handle();
        let root = ws.root_cwd().unwrap();
        std::fs::write(root.join("regular.txt"), b"regular").unwrap();
        std::fs::create_dir(root.join("real_dir")).unwrap();
        std::fs::write(root.join("real_dir/inside.txt"), b"inside").unwrap();
        std::os::unix::fs::symlink("regular.txt", root.join("linked_file.txt")).unwrap();
        std::os::unix::fs::symlink("real_dir", root.join("linked_dir")).unwrap();
        std::os::unix::fs::symlink("missing.txt", root.join("dangling.txt")).unwrap();

        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), b"secret").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("escape_dir")).unwrap();

        let req = FsListReq {
            depth: 2,
            ..list_req("")
        };
        let full = req.execute(&ws, None).await.unwrap();
        let regular = full
            .nodes
            .iter()
            .find(|node| node.name == "regular.txt")
            .unwrap();
        assert_eq!(regular.node_type, FsNodeType::File);
        assert_eq!(regular.size, Some(7));
        assert_eq!(regular.is_symlink, None);
        let directory = full
            .nodes
            .iter()
            .find(|node| node.name == "real_dir")
            .unwrap();
        assert_eq!(directory.node_type, FsNodeType::Directory);
        let linked_file = full
            .nodes
            .iter()
            .find(|node| node.name == "linked_file.txt")
            .unwrap();
        assert_eq!(linked_file.node_type, FsNodeType::File);
        assert_eq!(linked_file.size, Some(7));
        assert_eq!(linked_file.is_symlink, Some(true));
        let linked_dir = full
            .nodes
            .iter()
            .find(|node| node.name == "linked_dir")
            .unwrap();
        assert_eq!(linked_dir.node_type, FsNodeType::Directory);
        assert_eq!(linked_dir.is_symlink, Some(true));
        assert!(!full.nodes.iter().any(|node| node.name == "dangling.txt"));
        assert!(
            !full
                .nodes
                .iter()
                .any(|node| node.path.contains("escape_dir"))
        );
        assert!(!full.nodes.iter().any(|node| node.name == "secret.txt"));

        let mut paged = Vec::new();
        for offset in (0..full.nodes.len()).step_by(2) {
            let page = FsListReq {
                limit: 2,
                offset: offset as u64,
                ..req.clone()
            }
            .execute(&ws, None)
            .await
            .unwrap();
            paged.extend(page.nodes);
        }
        assert_eq!(paged, full.nodes);
    }
}
