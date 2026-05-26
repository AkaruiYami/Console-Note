use std::fs;
use std::io;
use std::path::PathBuf;

use crate::config::Config;
use crate::note::Note;

pub struct Storage {
    notes_dir: PathBuf,
    counter_path: PathBuf,
}

impl Storage {
    pub fn new(config: &Config) -> Self {
        let counter_path = config.config_dir.join("counter");
        Storage {
            notes_dir: config.notes_dir.clone(),
            counter_path,
        }
    }

    fn note_path(&self, id: u64) -> PathBuf {
        self.notes_dir.join(format!("{}.md", id))
    }

    pub fn next_id(&self) -> io::Result<u64> {
        let id = if self.counter_path.exists() {
            let raw = fs::read_to_string(&self.counter_path)?;
            raw.trim().parse::<u64>().unwrap_or(1)
        } else {
            1
        };
        fs::write(&self.counter_path, (id + 1).to_string())?;
        Ok(id)
    }

    pub fn create(&self, note: &Note) -> io::Result<()> {
        let content = note.to_file_content();
        fs::write(self.note_path(note.id), content.as_bytes())
    }

    pub fn list(&self) -> io::Result<Vec<Note>> {
        let mut notes = Vec::new();
        if !self.notes_dir.exists() {
            return Ok(notes);
        }
        for entry in fs::read_dir(&self.notes_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.ends_with(".md") {
                continue;
            }
            let content = match fs::read_to_string(entry.path()) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if let Ok(note) = Note::from_file_content(&content) {
                notes.push(note);
            }
        }
        notes.sort_by_key(|n| n.id);
        Ok(notes)
    }

    pub fn read(&self, id: u64) -> io::Result<Note> {
        let path = self.note_path(id);
        let raw = fs::read_to_string(&path)?;
        Note::from_file_content(&raw)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.message))
    }

    pub fn update(&self, note: &Note) -> io::Result<()> {
        self.create(note)
    }

    pub fn delete(&self, id: u64) -> io::Result<()> {
        let path = self.note_path(id);
        if path.exists() {
            fs::remove_file(path)
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, "note not found"))
        }
    }
}
