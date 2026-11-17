# console_note

A simple command-line note-taking app written in Rust. Notes are stored as Markdown files and can be managed through a CLI or an interactive TUI.

## Features

- Interactive TUI (no arguments) and full CLI
- Search notes by title or content
- Edit notes in your preferred external editor
- In-app settings page (TUI: press `s`)
- Plain-text storage: one Markdown file per note
- Configurable editor, notes directory, and display options
- Automatic two-way sync to a private GitHub repository

## Install

### Windows installer

Download and run `console_note-windows-x64-setup.exe` from [the latest release](https://github.com/AkaruiYami/Console-Note/releases/latest). Launch Console Note from the Start Menu; it opens in a terminal. The installer is unsigned, so Windows may show a publisher warning.

### Scoop (Windows)

```powershell
scoop install https://raw.githubusercontent.com/AkaruiYami/Console-Note/main/bucket/console-note.json
```

### Install script

Windows:

```powershell
irm https://raw.githubusercontent.com/AkaruiYami/Console-Note/main/install.ps1 | iex
```

Linux (curl or wget):

```bash
curl -fsSL https://raw.githubusercontent.com/AkaruiYami/Console-Note/main/install.sh | sh
```

The script downloads the latest release, verifies the SHA256 checksum, and installs to `%USERPROFILE%\bin` (Windows) or `~/.local/bin` (Linux).

### Cargo

```bash
cargo install --git https://github.com/AkaruiYami/Console-Note
```

### Manual download

Download the latest Linux or Windows x64 archive from [GitHub Releases](https://github.com/AkaruiYami/Console-Note/releases/latest). Extract `console_note` or `console_note.exe` into a directory on your `PATH`. Download the newer archive and replace the binary to update. Notes and configuration remain in their configured locations.

## Build from source

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
| `console_note --set <option> <value>`     | Set `editor`, `notes_dir`, or `hide_extension` |
| `console_note --wipe`                     | Delete all notes (asks for confirmation) |
| `console_note sync setup <client-id> <OWNER/REPO\|new:REPO>` | Connect or create a private GitHub repository |
| `console_note sync now`                   | Sync immediately                         |
| `console_note sync status`                | Show the connected repository            |
| `console_note sync disconnect`            | Remove local sync settings and login     |

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
| `s`              | Settings page        |
| `S`              | Sync now             |
| `d`              | Delete note          |
| `r`              | Refresh              |
| `q` / `Esc`      | Quit / back          |
| `Ctrl+C`         | Quit                 |

## Configuration

The config file is written to `%APPDATA%\console_note\config` on Windows or `~/.config/console_note/config` on Unix.

```ini
editor=notepad
notes_dir=C:\Users\you\AppData\Roaming\console_note\notes
hide_extension=true
```

Options:

- `editor` — the editor command
- `notes_dir` — the notes directory (must be absolute)
- `hide_extension` — hide the `.md` suffix in note listings (`true`/`false`, default `true`)

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

## GitHub sync

Register a GitHub App in GitHub Developer settings and enable **Device flow**. Give it **Contents: Read and write** repository permission. To let Console Note create a repository, also grant **Repository creation: Write**. Install the app on your account and select the private repository you want to use. For a new repository, the app can access repositories it creates. Copy the app's **Client ID** (not its App ID or client secret).

Run `console_note sync setup CLIENT_ID OWNER/REPO` to connect an existing private repository, or `console_note sync setup CLIENT_ID new:console-notes` to create one. The app prints a GitHub verification URL and code. You can also open the TUI settings page and select **GitHub sync** for the same setup flow. The repository must be private. Notes are readable Markdown under its `notes/` directory. Use a dedicated repository for notes.

The app syncs on launch and after note changes. While the TUI is open, it also watches for direct edits to `.md` files. Offline edits stay local and upload on the next successful sync. Changes made on two devices to the same note are preserved as separate notes; the extra copy is marked `sync conflict`. Deleting all notes with `--wipe` also deletes synced notes after confirmation.

Login tokens are kept in the operating system credential store. If no credential store is available, setup fails without saving tokens in plain text. `sync disconnect` removes this device's credentials and sync settings but keeps its local notes and the GitHub repository. After disconnecting, you can change `notes_dir` or connect a different repository.

On Linux, the credential store must provide the Secret Service API (`org.freedesktop.secrets`). If setup reports that the OS credential store is unavailable, enable and unlock a Secret Service provider before retrying. On Arch Linux, [KWallet](https://wiki.archlinux.org/title/KDE_Wallet) can provide this service; [GNOME Keyring](https://wiki.archlinux.org/title/GNOME/Keyring) is another option. From your normal desktop terminal, `busctl --user --list | rg org.freedesktop.secrets` shows whether the service is currently active. After enabling a provider, rerun `console_note sync setup` to get a new GitHub device code. No sync settings are saved when credential storage fails.

## Notes

- work best if NVIM is installed
- I always have a terminal ready and can be quickly open using a shortcut. That's why I made this so I could just pull my terminal then jog down notes.

## License

MIT
