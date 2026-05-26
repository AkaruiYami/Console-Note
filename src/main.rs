mod config;
mod note;
mod storage;

use std::env;
use std::fs;
use std::io;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use config::Config;
use note::Note;
use storage::Storage;

fn current_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    unix_to_iso(secs)
}

fn unix_to_iso(secs: u64) -> String {
    const SECS_PER_DAY: u64 = 86400;
    const SECS_PER_HOUR: u64 = 3600;
    const SECS_PER_MIN: u64 = 60;

    let days = secs / SECS_PER_DAY;
    let time = secs % SECS_PER_DAY;
    let hours = time / SECS_PER_HOUR;
    let minutes = (time % SECS_PER_HOUR) / SECS_PER_MIN;
    let seconds = time % SECS_PER_MIN;

    let (year, month, day) = civil_from_days(days as i64);

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}")
}

fn civil_from_days(z: i64) -> (i64, u64, u64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u64, d as u64)
}

fn edit_content(editor_cmd: &str, initial: &str) -> io::Result<String> {
    let tmp = env::temp_dir().join("console_note_edit.md");
    fs::write(&tmp, initial)?;

    let parts: Vec<&str> = editor_cmd.split_whitespace().collect();
    let status = Command::new(parts[0])
        .args(&parts[1..])
        .arg(&tmp)
        .status()?;

    if !status.success() {
        return Err(io::Error::other("editor exited with error"));
    }

    let result = fs::read_to_string(&tmp)?;
    let _ = fs::remove_file(&tmp);
    Ok(result)
}

fn cmd_add(
    storage: &Storage,
    config: &Config,
    title: &str,
    content: Option<&str>,
) -> io::Result<()> {
    let body = match content {
        Some(c) => c.to_string(),
        None => edit_content(&config.editor, "")?,
    };

    if body.trim().is_empty() {
        eprintln!("error: no content provided, note not created");
        return Ok(());
    }

    let id = storage.next_id()?;
    let now = current_timestamp();
    let note = Note {
        id,
        title: title.to_string(),
        content: body,
        created_at: now.clone(),
        updated_at: now,
    };
    storage.create(&note)?;
    println!("Created note {}: {}", id, title);
    Ok(())
}

fn cmd_list(storage: &Storage) -> io::Result<()> {
    let notes = storage.list()?;
    if notes.is_empty() {
        println!("No notes found.");
        return Ok(());
    }
    println!("{:<5} | {:<30} | Created", "ID", "Title");
    println!("{}", "-".repeat(55));
    for n in &notes {
        let title = if n.title.len() > 28 {
            format!("{}..", &n.title[..28])
        } else {
            n.title.clone()
        };
        let date = if n.created_at.len() >= 10 {
            &n.created_at[..10]
        } else {
            &n.created_at
        };
        println!("{:<5} | {:<30} | {}", n.id, title, date);
    }
    Ok(())
}

fn cmd_view(storage: &Storage, id: u64) -> io::Result<()> {
    let note = storage.read(id)?;
    println!("id:        {}", note.id);
    println!("title:     {}", note.title);
    println!("created:   {}", note.created_at);
    println!("updated:   {}", note.updated_at);
    println!();
    println!("{}", note.content);
    Ok(())
}

fn cmd_edit(storage: &Storage, config: &Config, id: u64, content: Option<&str>) -> io::Result<()> {
    let mut note = storage.read(id)?;

    let new_body = match content {
        Some(c) => c.to_string(),
        None => edit_content(&config.editor, &note.content)?,
    };

    if new_body == note.content {
        println!("No changes made.");
        return Ok(());
    }

    note.content = new_body;
    note.updated_at = current_timestamp();
    storage.update(&note)?;
    println!("Updated note {}", id);
    Ok(())
}

fn cmd_delete(storage: &Storage, id: u64) -> io::Result<()> {
    storage.delete(id)?;
    println!("Deleted note {}", id);
    Ok(())
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  console_note add <title> [-c <content>]");
    eprintln!("  console_note list");
    eprintln!("  console_note view <id>");
    eprintln!("  console_note edit <id> [-c <content>]");
    eprintln!("  console_note delete <id>");
}

fn main() -> io::Result<()> {
    let config = Config::load_or_create()?;
    let storage = Storage::new(&config);

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    match args[1].as_str() {
        "add" => {
            if args.len() < 3 {
                eprintln!("Usage: console_note add <title> [-c <content>]");
                return Ok(());
            }
            let title = &args[2];
            let content = if args.len() > 3 && args[3] == "-c" {
                args.get(4).map(|s| s.as_str())
            } else {
                None
            };
            cmd_add(&storage, &config, title, content)?;
        }
        "list" | "ls" => cmd_list(&storage)?,
        "view" => {
            if args.len() < 3 {
                eprintln!("Usage: console_note view <id>");
                return Ok(());
            }
            let id: u64 = args[2].parse().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "invalid ID, must be a number")
            })?;
            cmd_view(&storage, id)?;
        }
        "edit" => {
            if args.len() < 3 {
                eprintln!("Usage: console_note edit <id> [-c <content>]");
                return Ok(());
            }
            let id: u64 = args[2].parse().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "invalid ID, must be a number")
            })?;
            let content = if args.len() > 3 && args[3] == "-c" {
                args.get(4).map(|s| s.as_str())
            } else {
                None
            };
            cmd_edit(&storage, &config, id, content)?;
        }
        "delete" | "rm" => {
            if args.len() < 3 {
                eprintln!("Usage: console_note delete <id>");
                return Ok(());
            }
            let id: u64 = args[2].parse().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "invalid ID, must be a number")
            })?;
            cmd_delete(&storage, id)?;
        }
        _ => print_usage(),
    }

    Ok(())
}
