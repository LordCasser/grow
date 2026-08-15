use super::*;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

fn tree() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("symlink");
}

#[cfg(windows)]
fn symlink_dir(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_dir(target, link).expect("symlink dir");
}

#[cfg(windows)]
fn symlink_file(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_file(target, link).expect("symlink file");
}

/// Recursive `relative path -> bytes` snapshot used to prove a scan is
/// side-effect free.
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).expect("read_dir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            let rel = path
                .strip_prefix(root)
                .expect("strip prefix")
                .display()
                .to_string();
            let meta = fs::symlink_metadata(&path).expect("symlink_metadata");
            if meta.file_type().is_dir() {
                walk(&path, root, out);
            } else {
                out.insert(rel, fs::read(&path).expect("read"));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// A plausible on-disk SQLite database (valid 100-byte header, one page).
/// The scan must never open it, so the fixture only needs to look like one.
fn sqlite_shaped_bytes() -> Vec<u8> {
    let mut bytes = vec![0u8; 4096];
    bytes[..16].copy_from_slice(b"SQLite format 3\0");
    bytes[16..18].copy_from_slice(&4096u16.to_be_bytes());
    bytes[18] = 1; // legacy file format
    bytes[19] = 1; // read version
    bytes
}

#[test]
fn symlinks_are_billed_as_entries_and_never_followed() {
    let temp = tree();
    let root = temp.path().join("grow");
    fs::create_dir(&root).unwrap();
    let dir = root.join("dir");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("payload.bin"), vec![7u8; 100_000]).unwrap();
    let plain = root.join("plain.bin");
    fs::write(&plain, vec![9u8; 50_000]).unwrap();
    let link_to_dir = root.join("link-to-dir");
    symlink(&dir, &link_to_dir);
    let link_to_file = root.join("link-to-file");
    symlink(&plain, &link_to_file);
    let dangling = root.join("dangling");
    symlink(&root.join("missing-target"), &dangling);

    let report = scan(&root).unwrap();
    let entry = |name: &str| {
        report
            .entries
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("missing entry {name}"))
    };
    let dir_size = entry("dir").bytes;
    let plain_size = entry("plain.bin").bytes;
    let link_dir_size = entry("link-to-dir").bytes;
    let link_file_size = entry("link-to-file").bytes;
    let dangling_size = entry("dangling").bytes;

    // The link entries bill only the link itself, never the target content.
    assert!(
        link_dir_size < dir_size,
        "dir symlink must not include target contents"
    );
    assert!(
        link_file_size < plain_size,
        "file symlink must not include target contents"
    );
    // Independent billing: links match their own symlink_metadata.
    let bill_link = |p: &Path| bill(&fs::symlink_metadata(p).unwrap());
    assert_eq!(link_dir_size, bill_link(&link_to_dir));
    assert_eq!(link_file_size, bill_link(&link_to_file));
    assert_eq!(dangling_size, bill_link(&dangling));
    // Nothing double counted: following links would inflate the total.
    assert_eq!(
        report.total_bytes,
        dir_size + plain_size + link_dir_size + link_file_size + dangling_size
    );
    // A real directory does descend into its payload.
    assert!(dir_size > bill(&fs::symlink_metadata(&dir).unwrap()));
    assert!(report.warnings.is_empty());
}

#[test]
fn scan_is_metadata_only_and_creates_no_sqlite_sidecars() {
    let temp = tree();
    let root = temp.path().join("grow");
    fs::create_dir(&root).unwrap();
    let sessions = root.join("sessions");
    fs::create_dir(&sessions).unwrap();
    fs::write(sessions.join("chat.db"), sqlite_shaped_bytes()).unwrap();
    fs::write(sessions.join("chat.db-wal"), b"existing wal contents").unwrap();
    fs::write(sessions.join("chat.db-shm"), b"existing shm contents").unwrap();

    let before = snapshot(&root);
    let report = scan(&root).unwrap();
    let after = snapshot(&root);

    assert_eq!(
        before, after,
        "scan must not create or remove any file, including -wal/-shm sidecars"
    );
    assert!(report.warnings.is_empty());
    // Pre-existing sidecars are ordinary files and stay counted.
    let sessions_size = report
        .entries
        .iter()
        .find(|e| e.name == "sessions")
        .expect("sessions entry")
        .bytes;
    let db_size = bill(&fs::symlink_metadata(sessions.join("chat.db")).unwrap());
    let wal_size = bill(&fs::symlink_metadata(sessions.join("chat.db-wal")).unwrap());
    let shm_size = bill(&fs::symlink_metadata(sessions.join("chat.db-shm")).unwrap());
    assert!(sessions_size >= db_size + wal_size + shm_size);
}

#[test]
fn json_output_carries_schema_version_and_sorted_entries() {
    let temp = tree();
    let root = temp.path().join("grow");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("small")).unwrap();
    fs::write(root.join("small").join("a.txt"), vec![1u8; 1_000]).unwrap();
    fs::create_dir(root.join("big")).unwrap();
    fs::write(root.join("big").join("b.txt"), vec![2u8; 10_000]).unwrap();

    let report = scan(&root).unwrap();
    let mut buf = Vec::new();
    write_report(&report, true, &mut buf).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&buf).unwrap();

    assert_eq!(json["schemaVersion"], SCHEMA_VERSION);
    assert_eq!(json["root"], root.display().to_string());
    assert_eq!(json["totalBytes"], serde_json::json!(report.total_bytes));
    let entries = json["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["name"], "big");
    assert_eq!(entries[1]["name"], "small");
    assert_eq!(
        entries[0]["bytes"],
        serde_json::json!(report.entries[0].bytes)
    );
    assert_eq!(
        entries[1]["bytes"],
        serde_json::json!(report.entries[1].bytes)
    );
    assert!(entries[0]["bytes"].as_u64().unwrap() > entries[1]["bytes"].as_u64().unwrap());
    assert!(json["warnings"].as_array().unwrap().is_empty());
}

#[test]
fn human_output_lists_total_and_entries() {
    let temp = tree();
    let root = temp.path().join("grow");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("sessions")).unwrap();
    fs::write(root.join("sessions").join("chat.db"), vec![5u8; 50_000]).unwrap();
    fs::write(root.join("config.toml"), vec![6u8; 100]).unwrap();

    let report = scan(&root).unwrap();
    let text = human::format(&report);

    assert!(text.starts_with(&format!("Grow home: {}\n", root.display())));
    assert!(text.contains("Total:"));
    assert!(
        text.contains("KiB"),
        "expected a KiB-scaled size in:\n{text}"
    );
    assert!(text.contains("sessions"));
    assert!(text.contains("config.toml"));
    assert!(text.ends_with('\n'));
}

#[cfg(unix)]
#[test]
fn unix_billing_uses_physical_blocks_not_logical_size() {
    use std::os::unix::fs::MetadataExt as _;

    let temp = tree();
    let file = temp.path().join("allocated.bin");
    fs::write(&file, vec![0u8; 100_000]).unwrap();
    let meta = fs::metadata(&file).unwrap();
    assert_eq!(bill(&meta), meta.blocks() * 512);
    assert!(
        bill(&meta) >= meta.len(),
        "block-based billing must cover the file length"
    );
}

#[test]
fn missing_grow_home_scans_empty() {
    let temp = tree();
    let missing = temp.path().join("no-such-home");
    let report = scan(&missing).unwrap();
    assert_eq!(report.total_bytes, 0);
    assert!(report.entries.is_empty());
    assert!(report.warnings.is_empty());
}

#[test]
fn walk_stops_at_volume_boundary() {
    let temp = tree();
    let root = temp.path().join("grow");
    fs::create_dir(&root).unwrap();
    let mounted = root.join("mounted");
    fs::create_dir(&mounted).unwrap();
    fs::write(mounted.join("payload.bin"), vec![3u8; 10_000]).unwrap();
    let local = root.join("local");
    fs::create_dir(&local).unwrap();
    fs::write(local.join("payload.bin"), vec![4u8; 10_000]).unwrap();

    // Simulate "mounted" living on another volume: everything else shares
    // the root's (zero) device identity.
    fn probe(_meta: &std::fs::Metadata, path: &Path) -> u64 {
        if path.file_name().and_then(|n| n.to_str()) == Some("mounted") {
            1
        } else {
            0
        }
    }
    let report = scan_with(&root, probe).unwrap();

    let mounted_size = report
        .entries
        .iter()
        .find(|e| e.name == "mounted")
        .expect("mounted entry")
        .bytes;
    let local_size = report
        .entries
        .iter()
        .find(|e| e.name == "local")
        .expect("local entry")
        .bytes;
    assert_eq!(
        mounted_size,
        bill(&fs::symlink_metadata(&mounted).unwrap()),
        "a directory on another volume is billed as itself only"
    );
    assert!(
        local_size > bill(&fs::symlink_metadata(&local).unwrap()),
        "a directory on the root volume descends into its contents"
    );
    assert!(report.warnings.is_empty());
}

#[test]
fn entries_sort_largest_first_with_name_tiebreak() {
    let temp = tree();
    let root = temp.path().join("grow");
    fs::create_dir(&root).unwrap();
    // Two identically sized files bill equally on the same filesystem,
    // pinning the name tiebreak without depending on directory inode sizes.
    fs::write(root.join("alpha.bin"), vec![1u8; 2_000]).unwrap();
    fs::write(root.join("beta.bin"), vec![1u8; 2_000]).unwrap();
    fs::write(root.join("gamma.bin"), vec![1u8; 40_000]).unwrap();

    let report = scan(&root).unwrap();
    let names: Vec<&str> = report.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names[0], "gamma.bin", "largest subtree first");
    assert_eq!(names[1], "alpha.bin");
    assert_eq!(names[2], "beta.bin");
    assert!(report.entries[0].bytes > report.entries[1].bytes);
    assert_eq!(report.entries[1].bytes, report.entries[2].bytes);
}
