//! Filesystem extension ops (`workspace.fs_*`) — the server-proxied backing
//! for the shell's `grow/fs/*` ACP extension methods.
//!
//! These mirror the pure functions that previously lived only in the
//! shell (`shell/src/session/file_system.rs`) so that, in proxy
//! mode, a `grow/fs/*` request executes on the *remote* workspace server
//! instead of the agent host. Each request type implements
//! [`WorkspaceOp`], so it runs in-process for local sessions and routes
//! over the server `workspace_rpc` tool for proxy sessions — identical wire
//! output either way.
//!
//! Path resolution: an absolute `path` is used directly; a relative
//! `path` is joined onto `cwd` (the per-session cwd the shell resolves
//! and sends) or, when absent, the workspace root.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
#[cfg(test)]
use base64::Engine;
use chrono::Utc;

use crate::error::{WorkspaceError, WorkspaceResult};
use crate::handle::WorkspaceHandle;
use crate::workspace_ops::WorkspaceOp;

// Canonical in workspace-types; re-exported for existing paths.
use workspace_types::rpc::fs::FsReadEncoding;
pub use workspace_types::rpc::fs::{
    FsDeleteFileReq, FsExistsData, FsExistsReq, FsListData, FsListNode, FsListReq, FsReadFileData,
    FsReadFileReq, FsWriteFileReq,
};

/// Resolve a request `path` to an absolute path. Absolute paths are used
/// directly; relative paths join `cwd` (the shell-resolved per-session
/// cwd) or, when absent, the workspace root.
fn resolve_abs(
    path: &str,
    cwd: &Option<PathBuf>,
    ws: &WorkspaceHandle,
) -> WorkspaceResult<PathBuf> {
    let p = Path::new(path);
    if p.is_absolute() {
        return Ok(p.to_path_buf());
    }
    let base = match cwd {
        Some(c) => c.clone(),
        None => ws.root_cwd()?,
    };
    Ok(base.join(p))
}

#[async_trait]
impl WorkspaceOp for FsListReq {
    async fn execute(
        &self,
        ws: &WorkspaceHandle,
        _session_id: Option<&str>,
    ) -> WorkspaceResult<Self::Response> {
        let abs_unconfined = resolve_abs(&self.path, &self.cwd, ws)?;
        let (abs, confine_root) = ws.confine_to_workspace_root(&abs_unconfined).await?;
        // Off-executor: `list` does synchronous walk + metadata syscalls.
        let req = self.clone();
        tokio::task::spawn_blocking(move || list(&abs, &req, confine_root))
            .await
            .map_err(|e| WorkspaceError::Operation(e.to_string()))?
    }
}

#[async_trait]
impl WorkspaceOp for FsExistsReq {
    async fn execute(
        &self,
        ws: &WorkspaceHandle,
        _session_id: Option<&str>,
    ) -> WorkspaceResult<Self::Response> {
        let abs_unconfined = resolve_abs(&self.path, &self.cwd, ws)?;
        let (abs, _) = ws.confine_to_workspace_root(&abs_unconfined).await?;
        let exists = tokio::fs::try_exists(&abs).await.unwrap_or(false);
        Ok(FsExistsData { exists })
    }
}

#[async_trait]
impl WorkspaceOp for FsReadFileReq {
    async fn execute(
        &self,
        ws: &WorkspaceHandle,
        _session_id: Option<&str>,
    ) -> WorkspaceResult<Self::Response> {
        let abs_unconfined = resolve_abs(&self.path, &self.cwd, ws)?;
        let (abs, _) = ws.confine_to_workspace_root(&abs_unconfined).await?;

        // Binary-safe ranged read: `size` is the full file size, the
        // chunk is `[offset, offset + min(length, max_bytes, cap))`.
        let md = tokio::fs::metadata(&abs)
            .await
            .map_err(|e| WorkspaceError::Operation(e.to_string()))?;
        if md.is_dir() {
            return Err(WorkspaceError::Operation(format!(
                "not a file: {}",
                self.path
            )));
        }
        // Best-effort snapshot: a concurrent truncate/grow between here and
        // read_range can make `size` inconsistent with the returned chunk.
        let size = md.len();
        let offset = self.offset.unwrap_or(0);
        let length = super::walk::clamp_read_length(self.length, self.max_bytes);
        let chunk = super::walk::read_range(&abs, offset, length)
            .await
            .map_err(|e| WorkspaceError::Operation(e.to_string()))?;
        Ok(build_ranged_entry(chunk, size, self.encoding))
    }
}

#[async_trait]
impl WorkspaceOp for FsWriteFileReq {
    async fn execute(
        &self,
        ws: &WorkspaceHandle,
        _session_id: Option<&str>,
    ) -> WorkspaceResult<Self::Response> {
        let abs_unconfined = resolve_abs(&self.path, &self.cwd, ws)?;
        let (abs, _) = ws.confine_to_workspace_root(&abs_unconfined).await?;
        let content = self.content.clone();
        let create_dirs = self.create_dirs;
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            if create_dirs && let Some(parent) = abs.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&abs, content.as_bytes())
        })
        .await
        .map_err(|e| WorkspaceError::Operation(e.to_string()))?
        .map_err(|e| WorkspaceError::Operation(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl WorkspaceOp for FsDeleteFileReq {
    async fn execute(
        &self,
        ws: &WorkspaceHandle,
        _session_id: Option<&str>,
    ) -> WorkspaceResult<Self::Response> {
        let abs_unconfined = resolve_abs(&self.path, &self.cwd, ws)?;
        let (abs, _) = ws.confine_to_workspace_root(&abs_unconfined).await?;
        tokio::fs::remove_file(&abs)
            .await
            .map_err(|e| WorkspaceError::Operation(e.to_string()))?;
        Ok(())
    }
}

// =========================================================================
// Pure helpers — ported verbatim from the shell so output is identical.
// =========================================================================

fn list(
    abs_path: &Path,
    req: &FsListReq,
    confine_to_canonical_root: Option<PathBuf>,
) -> WorkspaceResult<FsListData> {
    // Confined to the canonical root when set (escaping symlinks not enumerated);
    // `None` (the default) walks unconfined.
    let page = super::walk::list_directory_paged(
        abs_path,
        super::walk::ListOptions {
            depth: req.depth,
            follow_symlinks: req.follow_symlinks,
            respect_git_ignore: req.respect_git_ignore,
            include_hidden: req.include_hidden,
            include_globs: &req.include_globs,
            exclude_globs: &req.exclude_globs,
            offset: req.offset,
            limit: req.limit,
            confine_to_canonical_root,
        },
        super::walk::MAX_LIST_COLLECT,
    );

    let nodes: Vec<FsListNode> = page
        .entries
        .into_iter()
        .map(|e| FsListNode {
            node_type: if e.is_dir { "directory" } else { "file" }.to_string(),
            size: e.size,
            modified_at: e.modified.map(|st| {
                let dt: chrono::DateTime<Utc> = st.into();
                dt.to_rfc3339()
            }),
            is_symlink: e.is_symlink.then_some(true),
            path: e.abs_path.to_string_lossy().into_owned(),
            name: e.name,
        })
        .collect();

    Ok(FsListData {
        nodes,
        truncated: page.truncated,
    })
}

/// Map a binary-safe ranged chunk to the shell-facing `FsReadFileData`.
/// `size` is the full file size; `lineCount` is omitted for ranged reads
/// and the MIME `type` is a coarse text/binary tag (mid-file chunks make
/// magic-byte sniffing meaningless).
fn build_ranged_entry(chunk: Vec<u8>, size: u64, encoding: FsReadEncoding) -> FsReadFileData {
    let (payload, is_text) = super::walk::encode_chunk(chunk, encoding);
    let (content, content_base64) = match payload {
        super::walk::ChunkPayload::Text(t) => (t, None),
        super::walk::ChunkPayload::Base64(b) => (String::new(), Some(b)),
    };
    FsReadFileData {
        content,
        content_base64,
        size,
        line_count: None,
        content_type: if is_text {
            "text/plain".to_string()
        } else {
            "application/octet-stream".to_string()
        },
    }
}

// =========================================================================
// Tests for the pure helpers (no `WorkspaceHandle` required).
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an `FsListReq` for a temp dir test. `path`/`cwd` are unused by
    /// `list` (it takes the resolved abs path directly); `respect_git_ignore`
    /// is off so the temp dir's location can't filter out our fixtures.
    fn list_req(limit: usize) -> FsListReq {
        FsListReq {
            path: String::new(),
            cwd: None,
            depth: 1,
            limit,
            offset: 0,
            include_hidden: true,
            follow_symlinks: true,
            respect_git_ignore: false,
            include_globs: Vec::new(),
            exclude_globs: Vec::new(),
        }
    }

    #[test]
    fn list_enumerates_nodes_without_truncation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join("a.txt"), b"a").expect("write a");
        std::fs::write(root.join("b.txt"), b"bb").expect("write b");
        std::fs::create_dir(root.join("sub")).expect("mkdir sub");

        let data = list(root, &list_req(1000), Some(root.to_path_buf())).expect("list");

        let names: Vec<&str> = data.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(data.nodes.len(), 3, "names: {names:?}");
        assert!(names.contains(&"a.txt"));
        assert!(names.contains(&"b.txt"));
        assert!(names.contains(&"sub"));
        assert!(!data.truncated);
        // Directories sort ahead of files.
        assert_eq!(data.nodes[0].name, "sub");
        assert_eq!(data.nodes[0].node_type, "directory");
    }

    #[test]
    fn list_marks_truncated_when_limit_reached() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join("a.txt"), b"a").expect("write a");
        std::fs::write(root.join("b.txt"), b"bb").expect("write b");
        std::fs::create_dir(root.join("sub")).expect("mkdir sub");

        let data = list(root, &list_req(1), Some(root.to_path_buf())).expect("list");
        assert_eq!(data.nodes.len(), 1);
        assert!(data.truncated);
    }

    #[test]
    fn list_paginates_with_offset() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        for n in ["a.txt", "b.txt", "c.txt", "d.txt"] {
            std::fs::write(root.join(n), b"x").expect("write");
        }
        let names = |d: FsListData| d.nodes.into_iter().map(|n| n.name).collect::<Vec<_>>();

        let mut req = list_req(2);
        let p0 = list(root, &req, Some(root.to_path_buf())).expect("list");
        assert!(p0.truncated);
        assert_eq!(names(p0), vec!["a.txt", "b.txt"]);

        req.offset = 2;
        let p1 = list(root, &req, Some(root.to_path_buf())).expect("list");
        assert!(!p1.truncated, "last page is not truncated");
        assert_eq!(names(p1), vec!["c.txt", "d.txt"]);

        // Offset past the end yields an empty, non-truncated page.
        req.offset = 10;
        let p2 = list(root, &req, Some(root.to_path_buf())).expect("list");
        assert!(!p2.truncated);
        assert!(p2.nodes.is_empty());
    }

    #[test]
    fn build_ranged_entry_encodes_utf8_and_binary() {
        // UTF-8 chunk under the default encoding → `content`, text/plain,
        // full `size` echoed, no line count.
        let e = build_ranged_entry(b"hello".to_vec(), 100, FsReadEncoding::Utf8);
        assert_eq!(e.content, "hello");
        assert!(e.content_base64.is_none());
        assert_eq!(e.size, 100);
        assert!(e.line_count.is_none());
        assert_eq!(e.content_type, "text/plain");

        // Explicit base64 of valid UTF-8 stays text/plain but travels in
        // `contentBase64`.
        let e = build_ranged_entry(b"hi".to_vec(), 2, FsReadEncoding::Base64);
        assert!(e.content.is_empty());
        assert_eq!(
            e.content_base64,
            Some(base64::engine::general_purpose::STANDARD.encode(b"hi")),
        );
        assert_eq!(e.content_type, "text/plain");

        // Non-UTF-8 bytes fall back to base64 + octet-stream.
        let raw = vec![0xff_u8, 0x00, 0xfe];
        let e = build_ranged_entry(raw.clone(), 3, FsReadEncoding::Utf8);
        assert!(e.content.is_empty());
        assert_eq!(
            e.content_base64,
            Some(base64::engine::general_purpose::STANDARD.encode(&raw)),
        );
        assert_eq!(e.content_type, "application/octet-stream");
    }
}
