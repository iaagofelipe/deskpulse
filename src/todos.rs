// src/todos.rs - To-Do Store with atomic persistence and external-edit reload

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static TMP_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoItem {
    pub id: String,
    pub text: String,
    pub done: bool,
    pub created_at_ms: u64,
}

#[derive(Debug)]
struct DiskMetadata {
    len: u64,
    modified: Option<SystemTime>,
}

#[derive(Debug, Default)]
struct TodoInner {
    last_meta: Option<DiskMetadata>,
    items: Vec<TodoItem>,
}

#[derive(Debug)]
pub struct TodoStore {
    path: PathBuf,
    inner: Mutex<TodoInner>,
}

impl TodoStore {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let mut inner = TodoInner::default();
        if path.exists() {
            let meta = get_disk_metadata(&path)?;
            let raw = fs::read_to_string(&path)
                .map_err(|e| format!("failed to read todos file {}: {e}", path.display()))?;
            let items: Vec<TodoItem> = if raw.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(&raw)
                    .map_err(|e| format!("failed to parse todos json {}: {e}", path.display()))?
            };
            inner.last_meta = Some(meta);
            inner.items = items;
        } else {
            inner.last_meta = None;
            inner.items = Vec::new();
        }

        Ok(Self {
            path,
            inner: Mutex::new(inner),
        })
    }

    pub fn list(&self) -> Vec<TodoItem> {
        let mut guard = match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let _ = self.sync_from_disk_if_modified(&mut guard);
        guard.items.clone()
    }

    pub fn add(&self, text: &str) -> Result<TodoItem, String> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err("todo text cannot be empty".to_string());
        }

        let mut guard = self
            .inner
            .lock()
            .map_err(|_| "todos store lock poisoned".to_string())?;
        self.sync_from_disk_if_modified(&mut guard)?;

        let created_at_ms = now_epoch_ms();
        let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let id = format!("{}-{}", created_at_ms, seq);

        let item = TodoItem {
            id,
            text: trimmed.to_string(),
            done: false,
            created_at_ms,
        };

        guard.items.push(item.clone());

        if let Err(e) = self.save_to_disk(&mut guard) {
            // rollback
            guard.items.pop();
            return Err(e);
        }

        Ok(item)
    }

    pub fn toggle(&self, id_or_index: &str) -> Result<Option<TodoItem>, String> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| "todos store lock poisoned".to_string())?;
        self.sync_from_disk_if_modified(&mut guard)?;

        let idx_opt = resolve_index_or_id(&guard.items, id_or_index);
        let Some(idx) = idx_opt else {
            return Ok(None);
        };

        let current_done = guard.items[idx].done;
        guard.items[idx].done = !current_done;

        if let Err(e) = self.save_to_disk(&mut guard) {
            // rollback
            guard.items[idx].done = current_done;
            return Err(e);
        }

        Ok(Some(guard.items[idx].clone()))
    }

    pub fn set_done(&self, id_or_index: &str, done: bool) -> Result<Option<TodoItem>, String> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| "todos store lock poisoned".to_string())?;
        self.sync_from_disk_if_modified(&mut guard)?;

        let idx_opt = resolve_index_or_id(&guard.items, id_or_index);
        let Some(idx) = idx_opt else {
            return Ok(None);
        };

        let prev_done = guard.items[idx].done;
        guard.items[idx].done = done;

        if let Err(e) = self.save_to_disk(&mut guard) {
            // rollback
            guard.items[idx].done = prev_done;
            return Err(e);
        }

        Ok(Some(guard.items[idx].clone()))
    }

    pub fn remove(&self, id_or_index: &str) -> Result<bool, String> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| "todos store lock poisoned".to_string())?;
        self.sync_from_disk_if_modified(&mut guard)?;

        let idx_opt = resolve_index_or_id(&guard.items, id_or_index);
        let Some(idx) = idx_opt else {
            return Ok(false);
        };

        let removed = guard.items.remove(idx);

        if let Err(e) = self.save_to_disk(&mut guard) {
            // rollback
            guard.items.insert(idx, removed);
            return Err(e);
        }

        Ok(true)
    }

    pub fn clear_done(&self) -> Result<usize, String> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| "todos store lock poisoned".to_string())?;
        self.sync_from_disk_if_modified(&mut guard)?;

        let original_items = guard.items.clone();
        let before_count = guard.items.len();
        guard.items.retain(|item| !item.done);
        let cleared = before_count - guard.items.len();

        if cleared == 0 {
            return Ok(0);
        }

        if let Err(e) = self.save_to_disk(&mut guard) {
            // rollback
            guard.items = original_items;
            return Err(e);
        }

        Ok(cleared)
    }

    fn sync_from_disk_if_modified(&self, inner: &mut TodoInner) -> Result<(), String> {
        if !self.path.exists() {
            if inner.last_meta.is_some() {
                inner.last_meta = None;
                inner.items.clear();
            }
            return Ok(());
        }

        let disk_meta = get_disk_metadata(&self.path)?;
        let changed = match &inner.last_meta {
            Some(last) => last.len != disk_meta.len || last.modified != disk_meta.modified,
            None => true,
        };

        if changed {
            let raw = fs::read_to_string(&self.path)
                .map_err(|e| format!("failed to read todos from disk {}: {e}", self.path.display()))?;
            let items: Vec<TodoItem> = if raw.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(&raw)
                    .map_err(|e| format!("failed to parse todos json {}: {e}", self.path.display()))?
            };
            inner.last_meta = Some(disk_meta);
            inner.items = items;
        }

        Ok(())
    }

    fn save_to_disk(&self, inner: &mut TodoInner) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create dir {}: {e}", parent.display()))?;
        }

        let json = serde_json::to_string_pretty(&inner.items)
            .map_err(|e| format!("failed to serialize todos json: {e}"))?;

        let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let file_name = self
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("todos.json");
        let tmp_name = format!(".{}.tmp.{}.{}", file_name, std::process::id(), seq);
        let tmp_path = self.path.with_file_name(tmp_name);

        fs::write(&tmp_path, json.as_bytes())
            .map_err(|e| format!("failed to write temp todos file {}: {e}", tmp_path.display()))?;

        let mut last_err = None;
        for attempt in 0..5 {
            match fs::rename(&tmp_path, &self.path) {
                Ok(()) => {
                    let meta = get_disk_metadata(&self.path)?;
                    inner.last_meta = Some(meta);
                    return Ok(());
                }
                Err(e) => {
                    last_err = Some(e);
                    if attempt < 4 {
                        std::thread::sleep(Duration::from_millis(25));
                    }
                }
            }
        }

        let _ = fs::remove_file(&tmp_path);
        Err(format!(
            "failed to rename temp file to {}: {}",
            self.path.display(),
            last_err.expect("rename failed at least once")
        ))
    }
}

fn resolve_index_or_id(items: &[TodoItem], id_or_index: &str) -> Option<usize> {
    let trimmed = id_or_index.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Exact id matching takes precedence over 1-based index fallback
    if let Some(pos) = items.iter().position(|item| item.id == trimmed) {
        return Some(pos);
    }

    // 1-based index fallback
    if let Ok(idx_1based) = trimmed.parse::<usize>() {
        if idx_1based >= 1 && idx_1based <= items.len() {
            return Some(idx_1based - 1);
        }
    }

    None
}

fn get_disk_metadata(path: &Path) -> Result<DiskMetadata, String> {
    let meta = fs::metadata(path)
        .map_err(|e| format!("failed to get metadata for {}: {e}", path.display()))?;
    Ok(DiskMetadata {
        len: meta.len(),
        modified: meta.modified().ok(),
    })
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_todo_crud_persists_atomically_and_supports_1based_index_or_id() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todos.json");

        let store = TodoStore::open(path.clone()).expect("open empty store");
        assert_eq!(store.list(), vec![]);

        // Reject blank text
        let blank_err = store.add("   ");
        assert!(blank_err.is_err(), "blank text should be rejected");

        // Add 3 items
        let item1 = store.add("Buy groceries").expect("add item 1");
        assert_eq!(item1.text, "Buy groceries");
        assert!(!item1.done);
        assert!(!item1.id.is_empty());

        let item2 = store.add("Fix audio backend").expect("add item 2");
        let item3 = store.add("Deploy deskpulse").expect("add item 3");

        let items = store.list();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].id, item1.id);
        assert_eq!(items[1].id, item2.id);
        assert_eq!(items[2].id, item3.id);

        // Toggle by 1-based index (item 1 -> done)
        let toggled = store.toggle("1").expect("toggle 1");
        assert!(toggled.is_some());
        let toggled = toggled.unwrap();
        assert_eq!(toggled.id, item1.id);
        assert!(toggled.done);
        assert!(store.list()[0].done);

        // Toggle back by ID
        let toggled_back = store.toggle(&item1.id).expect("toggle by id");
        assert!(toggled_back.is_some());
        assert!(!toggled_back.unwrap().done);
        assert!(!store.list()[0].done);

        // Set done explicitly by 1-based index and ID
        let set_res = store.set_done("2", true).expect("set_done index 2");
        assert!(set_res.is_some());
        assert!(set_res.unwrap().done);
        assert!(store.list()[1].done);

        let set_res2 = store.set_done(&item3.id, true).expect("set_done item 3 by id");
        assert!(set_res2.is_some());
        assert!(set_res2.unwrap().done);
        assert!(store.list()[2].done);

        // Reopen store from disk and check persistence
        let reopened = TodoStore::open(path.clone()).expect("reopen store");
        let reloaded_items = reopened.list();
        assert_eq!(reloaded_items.len(), 3);
        assert!(!reloaded_items[0].done);
        assert!(reloaded_items[1].done);
        assert!(reloaded_items[2].done);

        // Clear done (items 2 and 3 should be removed)
        let cleared = reopened.clear_done().expect("clear_done");
        assert_eq!(cleared, 2);
        let items_after_clear = reopened.list();
        assert_eq!(items_after_clear.len(), 1);
        assert_eq!(items_after_clear[0].id, item1.id);

        // Remove by 1-based index
        let removed = reopened.remove("1").expect("remove 1");
        assert!(removed);
        assert_eq!(reopened.list().len(), 0);

        // Non-existent remove
        let not_found_remove = reopened.remove("1").expect("remove on empty");
        assert!(!not_found_remove);
        let not_found_id = reopened.remove("non-existent-id").expect("remove invalid id");
        assert!(!not_found_id);
    }

    #[test]
    fn test_todo_store_reloads_when_external_writer_updates_file() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todos.json");

        let store = TodoStore::open(path.clone()).expect("open store");
        assert_eq!(store.list().len(), 0);

        let item1 = store.add("Internal item").expect("add internal");
        assert_eq!(store.list().len(), 1);

        // Simulate external CLI or editor writing to todos.json
        let external_items = vec![
            item1.clone(),
            TodoItem {
                id: "external-1".to_string(),
                text: "External task from CLI".to_string(),
                done: false,
                created_at_ms: now_epoch_ms(),
            },
        ];
        let serialized = serde_json::to_string_pretty(&external_items).expect("serialize external");

        // Small sleep to ensure modified time changes if filesystem has coarse timestamp resolution
        std::thread::sleep(Duration::from_millis(50));
        fs::write(&path, serialized.as_bytes()).expect("external write");

        // TodoStore::list() should detect disk modification and reload items
        let reloaded = store.list();
        assert_eq!(reloaded.len(), 2);
        assert_eq!(reloaded[0].id, item1.id);
        assert_eq!(reloaded[1].id, "external-1");
        assert_eq!(reloaded[1].text, "External task from CLI");

        // External modification toggle check: TodoStore operations also operate on reloaded state
        let toggled = store.toggle("2").expect("toggle external item by 1-based index");
        assert!(toggled.is_some());
        let toggled = toggled.unwrap();
        assert_eq!(toggled.id, "external-1");
        assert!(toggled.done);

        // Verify it was written back to disk
        let disk_content = fs::read_to_string(&path).expect("read after toggle");
        let on_disk: Vec<TodoItem> = serde_json::from_str(&disk_content).expect("parse disk");
        assert_eq!(on_disk.len(), 2);
        assert!(on_disk[1].done);
    }

    #[test]
    fn test_todo_exact_id_takes_precedence_over_1based_index() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todos.json");
        let initial_items = vec![
            TodoItem {
                id: "2".to_string(),
                text: "Item with numeric ID 2 at index 0".to_string(),
                done: false,
                created_at_ms: 1000,
            },
            TodoItem {
                id: "alpha".to_string(),
                text: "Second item in list".to_string(),
                done: false,
                created_at_ms: 1001,
            },
            TodoItem {
                id: "beta".to_string(),
                text: "Third item in list".to_string(),
                done: false,
                created_at_ms: 1002,
            },
        ];
        fs::write(
            &path,
            serde_json::to_string_pretty(&initial_items).unwrap(),
        )
        .unwrap();

        let store = TodoStore::open(path).expect("open store");

        // toggle("2") must match item with id == "2" (pos 0), NOT 1-based index 2 ("alpha")
        let toggled = store.toggle("2").expect("toggle").expect("found");
        assert_eq!(toggled.id, "2");
        assert!(toggled.done);
        let list = store.list();
        assert!(list[0].done);
        assert!(!list[1].done);

        // remove("2") must remove item with id == "2" (pos 0), leaving "alpha" and "beta"
        assert!(store.remove("2").expect("remove"));
        let remaining = store.list();
        assert_eq!(remaining.len(), 2);
        assert_eq!(remaining[0].id, "alpha");
        assert_eq!(remaining[1].id, "beta");
    }
}
