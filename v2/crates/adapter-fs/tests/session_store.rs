use adapter_fs::SessionStoreFs;
use core_ports::SessionStore;
use tempfile::tempdir;

#[test]
fn lifecycle() {
    let dir = tempdir().unwrap();
    let store = SessionStoreFs::new();
    let sid = "abc123";
    assert!(!store.is_initialized(dir.path(), sid).unwrap());
    store.mark_initialized(dir.path(), sid).unwrap();
    assert!(store.is_initialized(dir.path(), sid).unwrap());
    store.clear(dir.path(), sid).unwrap();
    assert!(!store.is_initialized(dir.path(), sid).unwrap());
}

#[test]
fn mark_is_idempotent() {
    let dir = tempdir().unwrap();
    let store = SessionStoreFs::new();
    store.mark_initialized(dir.path(), "s1").unwrap();
    store.mark_initialized(dir.path(), "s1").unwrap();
    assert!(store.is_initialized(dir.path(), "s1").unwrap());
}

#[test]
fn clear_missing_is_ok() {
    let dir = tempdir().unwrap();
    let store = SessionStoreFs::new();
    store.clear(dir.path(), "never-created").unwrap();
}

#[test]
fn multiple_sessions_are_independent() {
    let dir = tempdir().unwrap();
    let store = SessionStoreFs::new();
    store.mark_initialized(dir.path(), "s1").unwrap();
    assert!(store.is_initialized(dir.path(), "s1").unwrap());
    assert!(!store.is_initialized(dir.path(), "s2").unwrap());
    store.mark_initialized(dir.path(), "s2").unwrap();
    store.clear(dir.path(), "s1").unwrap();
    assert!(!store.is_initialized(dir.path(), "s1").unwrap());
    assert!(store.is_initialized(dir.path(), "s2").unwrap());
}

#[test]
fn flag_file_is_empty() {
    let dir = tempdir().unwrap();
    let store = SessionStoreFs::new();
    store.mark_initialized(dir.path(), "s1").unwrap();
    let path = dir.path().join(".init-s1");
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.is_empty());
}
