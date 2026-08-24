use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Config {
    pub editor: String,
    pub notes_dir: PathBuf,
    pub config_dir: PathBuf,
    pub hide_extension: bool,
}

pub(crate) fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "on" | "1" | "yes" => Some(true),
        "false" | "off" | "0" | "no" => Some(false),
        _ => None,
    }
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
        let mut config = Self::parse_content(config_dir, &raw);

        if let Ok(v) = env::var("CONSOLE_NOTE_EDITOR") {
            config.editor = v;
        }
        if let Ok(v) = env::var("CONSOLE_NOTE_NOTES_DIR") {
            config.notes_dir = PathBuf::from(v);
        }

        fs::create_dir_all(&config.notes_dir)?;

        Ok(config)
    }

    fn parse_content(config_dir: &Path, raw: &str) -> Self {
        let mut editor = Self::default_editor();
        let mut notes_dir = Self::default_notes_dir(config_dir);
        let mut hide_extension = true;

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
                    "hide_extension" => {
                        if let Some(v) = parse_bool(value) {
                            hide_extension = v;
                        }
                    }
                    _ => {}
                }
            }
        }

        Config {
            editor,
            notes_dir,
            config_dir: config_dir.to_path_buf(),
            hide_extension,
        }
    }

    pub fn save(&self) -> io::Result<()> {
        let path = self.config_dir.join("config");
        let content = format!(
            "# console_note config\neditor={}\nnotes_dir={}\nhide_extension={}\n",
            self.editor,
            self.notes_dir.display(),
            self.hide_extension
        );
        fs::write(path, content)
    }

    fn create_defaults(config_dir: &Path, config_path: &Path) -> io::Result<Self> {
        let editor = Self::default_editor();
        let notes_dir = Self::default_notes_dir(config_dir);

        fs::create_dir_all(&notes_dir)?;

        let content = format!(
            "# console_note config\neditor={}\nnotes_dir={}\nhide_extension=true\n",
            editor,
            notes_dir.display()
        );
        fs::write(config_path, content)?;

        Ok(Config {
            editor,
            notes_dir,
            config_dir: config_dir.to_path_buf(),
            hide_extension: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("console_note_config_{tag}"));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn create_defaults_enables_hide_extension() {
        let dir = test_config_dir("defaults");
        let config_path = dir.join("config");
        let config = Config::create_defaults(&dir, &config_path).unwrap();

        assert!(config.hide_extension);

        let raw = fs::read_to_string(&config_path).unwrap();
        assert!(raw.contains("hide_extension=true"));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_and_load_roundtrip_preserves_settings() {
        let dir = test_config_dir("roundtrip");
        let config_path = dir.join("config");

        let mut config = Config::create_defaults(&dir, &config_path).unwrap();
        config.editor = "vim".to_string();
        config.hide_extension = false;
        config.save().unwrap();

        let raw = fs::read_to_string(&config_path).unwrap();
        let reloaded = Config::parse_content(&dir, &raw);
        assert_eq!(reloaded.editor, "vim");
        assert!(!reloaded.hide_extension);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn parse_content_accepts_boolean_spelling_variants() {
        for (raw_value, expected) in [
            ("false", false),
            ("off", false),
            ("0", false),
            ("true", true),
            ("ON", true),
            ("yes", true),
        ] {
            let dir = test_config_dir(&format!("variants_{raw_value}"));
            let config = Config::parse_content(&dir, &format!("hide_extension={raw_value}\n"));
            assert_eq!(config.hide_extension, expected, "value {raw_value:?}");
        }
    }

    #[test]
    fn parse_content_falls_back_to_default_on_invalid_bool() {
        let dir = test_config_dir("invalid_bool");
        let config = Config::parse_content(&dir, "hide_extension=maybe\n");
        assert!(config.hide_extension);
    }
}
