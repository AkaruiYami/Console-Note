#[derive(Debug, Clone)]
pub struct Note {
    pub id: u64,
    pub title: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug)]
pub struct NoteParseError {
    pub message: String,
}

impl std::fmt::Display for NoteParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for NoteParseError {}

impl Note {
    pub fn from_file_content(raw: &str) -> Result<Self, NoteParseError> {
        let raw = raw.replace("\r\n", "\n");
        let s = raw.trim_start();

        if !s.starts_with("---") {
            return Err(NoteParseError {
                message: "missing opening frontmatter delimiter".into(),
            });
        }

        let rest = &s[3..];
        let end = rest.find("\n---").ok_or_else(|| NoteParseError {
            message: "missing closing frontmatter delimiter".into(),
        })?;

        let header = &rest[..end];
        let body = rest[end + 4..].trim_start().to_string();

        let mut id = None;
        let mut title = String::new();
        let mut created_at = String::new();
        let mut updated_at = String::new();

        for line in header.lines() {
            let line = line.trim();
            if let Some((key, value)) = line.split_once('=') {
                match key.trim() {
                    "id" => {
                        id = Some(value.trim().parse().map_err(|_| NoteParseError {
                            message: "invalid id field".into(),
                        })?);
                    }
                    "title" => title = value.trim().to_string(),
                    "created" => created_at = value.trim().to_string(),
                    "updated" => updated_at = value.trim().to_string(),
                    _ => {}
                }
            }
        }

        let id = id.ok_or_else(|| NoteParseError {
            message: "missing id field".into(),
        })?;

        Ok(Note {
            id,
            title,
            content: body,
            created_at,
            updated_at,
        })
    }

    pub fn to_file_content(&self) -> String {
        format!(
            "---\nid={}\ntitle={}\ncreated={}\nupdated={}\n---\n{}",
            self.id, self.title, self.created_at, self.updated_at, self.content
        )
    }
}
