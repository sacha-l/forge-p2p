use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A full note with content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub content: String,
    pub version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Summary metadata for listing notes.
#[derive(Debug, Clone)]
pub struct NoteMeta {
    pub id: String,
    pub title: String,
    pub version: u64,
    pub updated_at: DateTime<Utc>,
}

/// Manages notes as JSON files on disk.
pub struct NoteStore {
    notes_dir: PathBuf,
}

impl NoteStore {
    /// Create a new store backed by the given directory.
    pub fn new(data_dir: &Path) -> Result<Self> {
        let notes_dir = data_dir.join("notes");
        fs::create_dir_all(&notes_dir)
            .with_context(|| format!("failed to create notes directory: {}", notes_dir.display()))?;
        Ok(Self { notes_dir })
    }

    /// Create a new note with the given title and empty content.
    pub fn create(&self, title: &str) -> Result<Note> {
        let now = Utc::now();
        let note = Note {
            id: Uuid::new_v4().to_string(),
            title: title.to_string(),
            content: String::new(),
            version: 1,
            created_at: now,
            updated_at: now,
        };
        self.save(&note)?;
        Ok(note)
    }

    /// Update the content of an existing note. Bumps version.
    pub fn update(&self, id: &str, content: &str) -> Result<Note> {
        let mut note = self.get(id)?;
        note.content = content.to_string();
        note.version += 1;
        note.updated_at = Utc::now();
        self.save(&note)?;
        Ok(note)
    }

    /// Get a note by ID.
    pub fn get(&self, id: &str) -> Result<Note> {
        let path = self.note_path(id);
        let data = fs::read_to_string(&path)
            .with_context(|| format!("note not found: {id}"))?;
        let note: Note = serde_json::from_str(&data)
            .with_context(|| format!("failed to parse note: {id}"))?;
        Ok(note)
    }

    /// List metadata for all notes, sorted by updated_at descending.
    pub fn list(&self) -> Result<Vec<NoteMeta>> {
        let mut metas = Vec::new();
        let entries = fs::read_dir(&self.notes_dir)
            .with_context(|| "failed to read notes directory")?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                let data = fs::read_to_string(&path)?;
                if let Ok(note) = serde_json::from_str::<Note>(&data) {
                    metas.push(NoteMeta {
                        id: note.id,
                        title: note.title,
                        version: note.version,
                        updated_at: note.updated_at,
                    });
                }
            }
        }
        metas.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(metas)
    }

    /// Delete a note by ID.
    #[allow(dead_code)]
    pub fn delete(&self, id: &str) -> Result<()> {
        let path = self.note_path(id);
        fs::remove_file(&path)
            .with_context(|| format!("failed to delete note: {id}"))?;
        Ok(())
    }

    /// Save a note to disk (used internally and for syncing).
    pub fn save(&self, note: &Note) -> Result<()> {
        let path = self.note_path(&note.id);
        let data = serde_json::to_string_pretty(note)?;
        fs::write(&path, data)
            .with_context(|| format!("failed to write note: {}", note.id))?;
        Ok(())
    }

    fn note_path(&self, id: &str) -> PathBuf {
        self.notes_dir.join(format!("{id}.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_store() -> (NoteStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let store = NoteStore::new(dir.path()).unwrap();
        (store, dir)
    }

    #[test]
    fn create_and_read() {
        let (store, _dir) = temp_store();
        let note = store.create("My First Note").unwrap();
        assert_eq!(note.title, "My First Note");
        assert_eq!(note.version, 1);
        assert!(note.content.is_empty());

        let fetched = store.get(&note.id).unwrap();
        assert_eq!(fetched.id, note.id);
        assert_eq!(fetched.title, "My First Note");
    }

    #[test]
    fn update_bumps_version() {
        let (store, _dir) = temp_store();
        let note = store.create("Test").unwrap();
        assert_eq!(note.version, 1);

        let updated = store.update(&note.id, "Hello world").unwrap();
        assert_eq!(updated.version, 2);
        assert_eq!(updated.content, "Hello world");
        assert!(updated.updated_at >= note.updated_at);
    }

    #[test]
    fn list_notes() {
        let (store, _dir) = temp_store();
        store.create("Note A").unwrap();
        store.create("Note B").unwrap();

        let metas = store.list().unwrap();
        assert_eq!(metas.len(), 2);
    }

    #[test]
    fn delete_note() {
        let (store, _dir) = temp_store();
        let note = store.create("To Delete").unwrap();
        assert!(store.get(&note.id).is_ok());

        store.delete(&note.id).unwrap();
        assert!(store.get(&note.id).is_err());
    }

    #[test]
    fn get_nonexistent_note_fails() {
        let (store, _dir) = temp_store();
        assert!(store.get("nonexistent-id").is_err());
    }
}
