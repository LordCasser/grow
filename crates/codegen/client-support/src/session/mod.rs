use std::path::PathBuf;

pub mod info;

pub use info::Info;

// Re-export shared feedback wire types used by downstream crates
// (e.g. pager-render).

pub fn session_dir(info: &Info) -> PathBuf {
    tools::util::grow_home::sessions_cwd_dir(&info.cwd).join(info.id.to_string())
}
