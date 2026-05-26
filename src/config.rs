use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub struct Config {
    pub editor: String,
    pub notes_dir: PathBuf,
    pub config_dir: PathBuf,
}

impl Config {
    fn default_config_dir() -> PathBuf {
        if let Ok(appdata) = env::var("APPDATA") {
            PathBuf::from(appdata).join("console_note")
        } else if let Ok(home) = env::var("HOME") {
            PathBuf::from(home).join(".config").join("console_note")
        } else {
            PathBuf::from("./.console_note")
        }
    }

    fn default_editor() -> String {
        if cfg!(windows) {
            "notepad".to_string()
        } else {
            env::var("EDITOR").unwrap_or_else(|_| "nano".to_string())
        }
    }

    fn default_notes_dir(config_dir: &Path) -> PathBuf {
        config_dir.join("notes")
    }

    pub fn load_or_create() -> io::Result<Self> {
        let config_dir = Self::default_config_dir();
        let config_path = config_dir.join("config");

        if config_path.exists() {
            Self::load(&config_dir, &config_path)
        } else {
            Self::create_defaults(&config_dir, &config_path)
        }
    }

    fn load(config_dir: &Path, config_path: &Path) -> io::Result<Self> {
        let raw = fs::read_to_string(config_path)?;
        let mut editor = Self::default_editor();
        let mut notes_dir = Self::default_notes_dir(config_dir);

        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                match key.trim() {
                    "editor" => editor = value.trim().to_string(),
                    "notes_dir" => {
                        let p = PathBuf::from(value.trim());
                        if p.is_absolute() {
                            notes_dir = p;
                        }
                    }
                    _ => {}
                }
            }
        }

        if let Ok(v) = env::var("CONSOLE_NOTE_EDITOR") {
            editor = v;
        }
        if let Ok(v) = env::var("CONSOLE_NOTE_NOTES_DIR") {
            notes_dir = PathBuf::from(v);
        }

        fs::create_dir_all(&notes_dir)?;

        Ok(Config {
            editor,
            notes_dir,
            config_dir: config_dir.to_path_buf(),
        })
    }

    fn create_defaults(config_dir: &Path, config_path: &Path) -> io::Result<Self> {
        let editor = Self::default_editor();
        let notes_dir = Self::default_notes_dir(config_dir);

        fs::create_dir_all(&notes_dir)?;

        let content = format!(
            "# console_note config\neditor={}\nnotes_dir={}\n",
            editor,
            notes_dir.display()
        );
        fs::write(config_path, content)?;

        Ok(Config {
            editor,
            notes_dir,
            config_dir: config_dir.to_path_buf(),
        })
    }
}
