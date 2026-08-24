# console_note

A simple command-line note-taking app written in Rust. Notes are stored as Markdown files and can be managed through a CLI or an interactive TUI.

## Features

- Interactive TUI (no arguments) and full CLI
- Search notes by title or content
- Edit notes in your preferred external editor
- Plain-text storage: one Markdown file per note
- Configurable editor and notes directory

## Build

```bash
cargo build --release
```

The binary will be at `target/release/console_note`.

## Usage

Running without arguments opens the interactive TUI.

| Command                                   | Description                              |
| ----------------------------------------- | ---------------------------------------- |
| `console_note add <title> [-c <content>]` | Create a note (opens editor if no `-c`)  |
| `console_note list`                       | List all notes (`ls` works too)          |
| `console_note view <id>`                  | Show a note                              |
| `console_note edit <id> [-c <content>]`   | Edit a note (opens editor if no `-c`)    |
| `console_note rename <id> <new-title>`    | Rename a note (`mv` works too)           |
| `console_note delete <id>`                | Delete a note (`rm` works too)           |
| `console_note --set <option> <value>`     | Set `editor` or `notes_dir`              |
| `console_note --wipe`                     | Delete all notes (asks for confirmation) |

## TUI keys

| Key              | Action               |
| ---------------- | -------------------- |
| `↑`/`k`, `↓`/`j` | Move selection       |
| `g`, `G`         | Jump to first / last |
| `Enter`          | View note            |
| `/`              | Search               |
| `n`              | New note             |
| `e`              | Edit note            |
| `R`              | Rename note          |
| `d`              | Delete note          |
| `r`              | Refresh              |
| `q` / `Esc`      | Quit / back          |
| `Ctrl+C`         | Quit                 |

## Configuration

The config file is written to `%APPDATA%\console_note\config` on Windows or `~/.config/console_note/config` on Unix.

```ini
editor=notepad
notes_dir=C:\Users\you\AppData\Roaming\console_note\notes
```

Options can also be set via environment variables:

- `CONSOLE_NOTE_EDITOR` — the editor command
- `CONSOLE_NOTE_NOTES_DIR` — the notes directory (must be absolute)

## Storage

Each note is a Markdown file named `<id>.md` with frontmatter:

```
---
id=1
title=My note
created=2026-08-16 10:00:00
updated=2026-08-16 10:00:00
---
Note content here.
```

## Notes

- work best if NVIM is installed
- I always have a terminal ready and can be quickly open using a shortcut. That's why I made this so I could just pull my terminal then jog down notes.

## License

MIT
