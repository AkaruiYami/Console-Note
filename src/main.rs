mod config;
mod editor;
mod note;
mod storage;
mod time;
mod tui;

use std::env;
use std::fs;
use std::io::{self, Write};

use config::Config;
use note::Note;
use storage::Storage;

use editor::edit_content;
use time::current_timestamp;

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

fn cmd_rename(storage: &Storage, id: u64, new_title: &str) -> io::Result<()> {
    let mut note = storage.read(id)?;

    if note.title == new_title {
        println!("No changes made.");
        return Ok(());
    }

    note.title = new_title.to_string();
    note.updated_at = current_timestamp();
    storage.update(&note)?;
    println!("Renamed note {}", id);
    Ok(())
}

fn cmd_delete(storage: &Storage, id: u64) -> io::Result<()> {
    storage.delete(id)?;
    println!("Deleted note {}", id);
    Ok(())
}

fn cmd_set(config: &mut Config, option: &str, value: &str) -> io::Result<()> {
    match option {
        "editor" => config.editor = value.to_string(),
        "notes_dir" => {
            let p = std::path::PathBuf::from(value);
            if !p.is_absolute() {
                eprintln!("error: notes_dir must be an absolute path");
                return Ok(());
            }
            fs::create_dir_all(&p)?;
            config.notes_dir = p;
        }
        _ => {
            eprintln!("error: unknown option '{option}' (valid: editor, notes_dir)");
            return Ok(());
        }
    }
    config.save()?;
    println!("Set {option} = {value}");
    Ok(())
}

fn cmd_wipe(storage: &Storage) -> io::Result<()> {
    print!("Are you sure you want to delete all notes? (y/N): ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    if !input.trim().eq_ignore_ascii_case("y") {
        println!("Aborted.");
        return Ok(());
    }
    let count = storage.wipe()?;
    if count == 0 {
        println!("No notes to wipe.");
    } else {
        println!("Wiped {} note(s).", count);
    }
    Ok(())
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  console_note add <title> [-c <content>]");
    eprintln!("  console_note list");
    eprintln!("  console_note view <id>");
    eprintln!("  console_note edit <id> [-c <content>]");
    eprintln!("  console_note rename <id> <new-title>");
    eprintln!("  console_note delete <id>");
    eprintln!("  console_note --set <option> <value>");
    eprintln!("  console_note --wipe");
}

fn main() -> io::Result<()> {
    let mut config = Config::load_or_create()?;
    let storage = Storage::new(&config);

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        return tui::run(&storage, &config);
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
        "rename" | "mv" => {
            if args.len() < 4 {
                eprintln!("Usage: console_note rename <id> <new-title>");
                return Ok(());
            }
            let id: u64 = args[2].parse().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "invalid ID, must be a number")
            })?;
            cmd_rename(&storage, id, &args[3])?;
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
        "--set" => {
            if args.len() < 4 {
                eprintln!("Usage: console_note --set <option> <value>");
                return Ok(());
            }
            cmd_set(&mut config, &args[2], &args[3])?;
        }
        "--wipe" => cmd_wipe(&storage)?,
        _ => print_usage(),
    }

    Ok(())
}
