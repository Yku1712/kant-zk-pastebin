// Persistent archive session store.
// Archive sessions (ARCHIVE_STORE) were in-memory only, so they were lost
// on every server restart. This module writes each session's ArchiveResult
// to disk as a JSON file so old archives can still be browsed.
use crate::archive::ArchiveResult;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

lazy_static::lazy_static! {
    /// Global cache: session_id -> ArchiveResult, populated lazily from disk.
    static ref MEMORY_CACHE: Mutex<HashMap<String, ArchiveResult>> =
        Mutex::new(HashMap::new());
}

/// Directory where archive session JSON files live.
fn sessions_dir(uucp_dir: &str) -> PathBuf {
    Path::new(uucp_dir).join("archive_sessions")
}

/// Ensure the sessions directory exists.
fn ensure_dir(uucp_dir: &str) -> std::io::Result<()> {
    fs::create_dir_all(sessions_dir(uucp_dir))
}

/// Persist an ArchiveResult to disk.
pub fn save_session(uucp_dir: &str, session_id: &str, result: &ArchiveResult) -> std::io::Result<()> {
    ensure_dir(uucp_dir)?;
    let path = sessions_dir(uucp_dir).join(format!("{}.json", session_id));
    let json = serde_json::to_string_pretty(result)?;
    fs::write(path, json)?;

    // Also update in-memory cache
    MEMORY_CACHE.lock().unwrap().insert(session_id.to_string(), result.clone());
    Ok(())
}

/// Load an ArchiveResult from disk (with in-memory caching).
pub fn load_session(uucp_dir: &str, session_id: &str) -> Option<ArchiveResult> {
    // Check memory cache first
    {
        let cache = MEMORY_CACHE.lock().unwrap();
        if let Some(r) = cache.get(session_id) {
            return Some(r.clone());
        }
    }

    // Load from disk
    let path = sessions_dir(uucp_dir).join(format!("{}.json", session_id));
    let data = fs::read_to_string(path).ok()?;
    let result: ArchiveResult = serde_json::from_str(&data).ok()?;

    // Update memory cache
    MEMORY_CACHE.lock().unwrap().insert(session_id.to_string(), result.clone());
    Some(result)
}

/// Load all sessions from disk into memory.
pub fn load_all_sessions(uucp_dir: &str) -> HashMap<String, ArchiveResult> {
    let dir = sessions_dir(uucp_dir);
    let mut sessions = HashMap::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(data) = fs::read_to_string(&path) {
                    if let Ok(result) = serde_json::from_str::<ArchiveResult>(&data) {
                        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                        sessions.insert(stem, result.clone());
                    }
                }
            }
        }
    }
    sessions
}

/// Check if a session exists on disk.
pub fn session_exists(uucp_dir: &str, session_id: &str) -> bool {
    MEMORY_CACHE.lock().unwrap().contains_key(session_id)
        || sessions_dir(uucp_dir)
            .join(format!("{}.json", session_id))
            .exists()
}