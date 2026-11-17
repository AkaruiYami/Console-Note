use std::io::{self, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use ratatui::layout::{Alignment, Constraint, Flex, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Wrap,
};
use ratatui::{DefaultTerminal, Frame};
use unicode_width::UnicodeWidthStr;

use crate::config::Config;
use crate::editor;
use crate::note::Note;
use crate::storage::Storage;
use crate::sync;
use crate::time::current_timestamp;

const POLL_TIMEOUT: Duration = Duration::from_millis(100);

pub fn run(storage: &Storage, config: &Config) -> io::Result<()> {
    let mut app = App::new(storage, config);
    let _watcher = start_sync_worker(&mut app, config)?;
    let mut terminal = ratatui::init();
    let result = run_app(&mut app, &mut terminal);
    ratatui::restore();
    result
}

fn run_app(app: &mut App, terminal: &mut DefaultTerminal) -> io::Result<()> {
    while !app.should_quit {
        if let Some(receiver) = &app.sync_result
            && let Ok(message) = receiver.try_recv()
        {
            app.reload();
            app.set_message(message);
        }
        terminal.draw(|frame| app.draw(frame))?;
        if event::poll(POLL_TIMEOUT)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && let Err(err) = app.handle_key(key, terminal)
        {
            app.set_message(format!("error: {err}"));
        }
    }
    Ok(())
}

fn start_sync_worker(app: &mut App<'_>, config: &Config) -> io::Result<RecommendedWatcher> {
    let (request_tx, request_rx) = mpsc::channel::<()>();
    let (result_tx, result_rx) = mpsc::channel::<String>();
    let watcher_tx = request_tx.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event
            && event
                .paths
                .iter()
                .any(|path| path.extension().is_some_and(|ext| ext == "md"))
        {
            let _ = watcher_tx.send(());
        }
    })
    .map_err(io::Error::other)?;
    watcher
        .watch(&config.notes_dir, RecursiveMode::NonRecursive)
        .map_err(io::Error::other)?;
    let worker_config = config.clone();
    std::thread::spawn(move || {
        while request_rx.recv().is_ok() {
            while request_rx.recv_timeout(Duration::from_millis(300)).is_ok() {}
            let message = match sync::run(&worker_config) {
                Ok(message) => message,
                Err(err) => format!("error: sync: {err}"),
            };
            if result_tx.send(message).is_err() {
                break;
            }
        }
    });
    app.sync_request = Some(request_tx.clone());
    app.sync_result = Some(result_rx);
    if sync::SyncState::load(config)?.is_some() {
        let _ = request_tx.send(());
    }
    Ok(watcher)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    List,
    View,
    TitleInput,
    ConfirmDelete,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum InputPurpose {
    NewNote,
    Rename(u64),
    EditorSetting,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SettingsRow {
    Editor,
    HideExtension,
    NotesDir,
    Sync,
}

impl SettingsRow {
    const COUNT: usize = 4;

    fn from_index(index: usize) -> Self {
        match index {
            0 => SettingsRow::Editor,
            1 => SettingsRow::HideExtension,
            2 => SettingsRow::NotesDir,
            _ => SettingsRow::Sync,
        }
    }
}

struct App<'a> {
    storage: &'a Storage,
    config: Config,
    notes: Vec<Note>,
    list_state: ListState,
    mode: Mode,
    filter: String,
    filter_active: bool,
    input: String,
    input_purpose: InputPurpose,
    input_origin: Mode,
    view_id: u64,
    view_scroll: u16,
    confirm_id: Option<u64>,
    settings_row: usize,
    status: String,
    should_quit: bool,
    sync_request: Option<Sender<()>>,
    sync_result: Option<Receiver<String>>,
}

impl<'a> App<'a> {
    fn new(storage: &'a Storage, config: &Config) -> Self {
        let mut app = App {
            storage,
            config: config.clone(),
            notes: Vec::new(),
            list_state: ListState::default(),
            mode: Mode::List,
            filter: String::new(),
            filter_active: false,
            input: String::new(),
            input_purpose: InputPurpose::NewNote,
            input_origin: Mode::List,
            view_id: 0,
            view_scroll: 0,
            confirm_id: None,
            settings_row: 0,
            status: String::new(),
            should_quit: false,
            sync_request: None,
            sync_result: None,
        };
        app.reload();
        app
    }

    fn set_message(&mut self, msg: String) {
        self.status = msg;
    }

    fn reload(&mut self) {
        self.notes = self.storage.list().unwrap_or_default();
        self.clamp_selection();
    }

    fn clamp_selection(&mut self) {
        let count = self.filtered().len();
        let selected = if count == 0 {
            None
        } else {
            let current = self.list_state.selected().unwrap_or(0);
            Some(current.min(count - 1))
        };
        self.list_state.select(selected);
    }

    fn filtered(&self) -> Vec<&Note> {
        let query = self.filter.trim().to_lowercase();
        if query.is_empty() {
            return self.notes.iter().collect();
        }
        self.notes
            .iter()
            .filter(|n| {
                n.title.to_lowercase().contains(&query) || n.content.to_lowercase().contains(&query)
            })
            .collect()
    }

    fn selected_note(&self) -> Option<&Note> {
        self.list_state
            .selected()
            .and_then(|i| self.filtered().get(i).copied())
    }

    fn selected_id(&self) -> Option<u64> {
        self.selected_note().map(|n| n.id)
    }

    fn handle_key(&mut self, key: KeyEvent, terminal: &mut DefaultTerminal) -> io::Result<()> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return Ok(());
        }

        match self.mode {
            Mode::List => self.handle_list_key(key, terminal),
            Mode::View => self.handle_view_key(key, terminal),
            Mode::TitleInput => self.handle_title_input_key(key, terminal),
            Mode::ConfirmDelete => self.handle_confirm_key(key),
            Mode::Settings
                if matches!(key.code, KeyCode::Enter | KeyCode::Char(' '))
                    && SettingsRow::from_index(self.settings_row) == SettingsRow::Sync =>
            {
                self.configure_or_sync(terminal)
            }
            Mode::Settings => self.handle_settings_key(key),
        }
    }

    fn handle_list_key(&mut self, key: KeyEvent, terminal: &mut DefaultTerminal) -> io::Result<()> {
        if self.filter_active {
            match key.code {
                KeyCode::Esc => {
                    self.filter.clear();
                    self.filter_active = false;
                    self.clamp_selection();
                }
                KeyCode::Enter => {
                    self.filter_active = false;
                }
                KeyCode::Backspace => {
                    self.filter.pop();
                    self.clamp_selection();
                }
                KeyCode::Char(c) => {
                    self.filter.push(c);
                    self.clamp_selection();
                }
                _ => {}
            }
            return Ok(());
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Home | KeyCode::Char('g') => self.list_state.select_first(),
            KeyCode::End | KeyCode::Char('G') => self.list_state.select_last(),
            KeyCode::Enter => self.enter_view(),
            KeyCode::Char('/') => {
                self.filter.clear();
                self.filter_active = true;
            }
            KeyCode::Char('n') => {
                self.input.clear();
                self.input_purpose = InputPurpose::NewNote;
                self.input_origin = Mode::List;
                self.mode = Mode::TitleInput;
            }
            KeyCode::Char('s') => {
                self.settings_row = 0;
                self.mode = Mode::Settings;
            }
            KeyCode::Char('R') => match self.selected_id() {
                Some(id) => self.start_rename(id),
                None => self.set_message("No note selected.".to_string()),
            },
            KeyCode::Char('e') => self.enter_edit(terminal)?,
            KeyCode::Char('d') | KeyCode::Delete => self.confirm_delete(),
            KeyCode::Char('r') => {
                self.reload();
                self.set_message("Refreshed.".to_string());
            }
            KeyCode::Char('S') => self.request_sync(),
            KeyCode::Esc | KeyCode::Char('q') => self.should_quit = true,
            _ => {}
        }
        Ok(())
    }

    fn move_selection(&mut self, delta: i64) {
        let count = self.filtered().len() as i64;
        if count == 0 {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0) as i64;
        let next = (current + delta).clamp(0, count - 1);
        self.list_state.select(Some(next as usize));
    }

    fn enter_view(&mut self) {
        if let Some(note) = self.selected_note() {
            self.view_id = note.id;
            self.view_scroll = 0;
            self.mode = Mode::View;
        }
    }

    fn confirm_delete(&mut self) {
        if let Some(id) = self.selected_id() {
            self.confirm_id = Some(id);
            self.mode = Mode::ConfirmDelete;
        }
    }

    fn handle_confirm_key(&mut self, key: KeyEvent) -> io::Result<()> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(id) = self.confirm_id.take() {
                    match self.storage.delete(id) {
                        Ok(()) => {
                            self.reload();
                            self.request_sync();
                            self.set_message(format!("Deleted note {id}."));
                        }
                        Err(err) => self.set_message(format!("error: {err}")),
                    }
                }
                self.mode = Mode::List;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.confirm_id = None;
                self.mode = Mode::List;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_title_input_key(
        &mut self,
        key: KeyEvent,
        terminal: &mut DefaultTerminal,
    ) -> io::Result<()> {
        match key.code {
            KeyCode::Enter => match self.input_purpose {
                InputPurpose::Rename(id) => {
                    self.input_purpose = InputPurpose::NewNote;
                    self.commit_rename(id)?;
                }
                InputPurpose::EditorSetting => {
                    self.input_purpose = InputPurpose::NewNote;
                    self.commit_editor_setting()?;
                }
                InputPurpose::NewNote => {
                    let title = self.input.trim().to_string();
                    self.mode = Mode::List;
                    if title.is_empty() {
                        self.set_message("No title provided; note not created.".to_string());
                        return Ok(());
                    }
                    self.create_note(&title, terminal)?;
                }
            },
            KeyCode::Esc => self.cancel_input(),
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) => self.input.push(c),
            _ => {}
        }
        Ok(())
    }

    fn handle_view_key(&mut self, key: KeyEvent, terminal: &mut DefaultTerminal) -> io::Result<()> {
        let page = terminal.size()?.height.saturating_sub(6).max(1);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.view_scroll = self.view_scroll.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.view_scroll = self.view_scroll.saturating_add(1)
            }
            KeyCode::PageUp => self.view_scroll = self.view_scroll.saturating_sub(page),
            KeyCode::PageDown => self.view_scroll = self.view_scroll.saturating_add(page),
            KeyCode::Home => self.view_scroll = 0,
            KeyCode::End => self.view_scroll = u16::MAX,
            KeyCode::Char('e') => self.enter_edit(terminal)?,
            KeyCode::Char('R') => self.start_rename(self.view_id),
            KeyCode::Esc | KeyCode::Backspace => {
                self.mode = Mode::List;
                self.clamp_selection();
            }
            KeyCode::Char('q') => self.should_quit = true,
            _ => {}
        }
        Ok(())
    }

    fn enter_edit(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        let id = match self.selected_id() {
            Some(id) => id,
            None => {
                self.set_message("No note selected.".to_string());
                return Ok(());
            }
        };
        let mut note = match self.storage.read(id) {
            Ok(note) => note,
            Err(err) => {
                self.set_message(format!("error: {err}"));
                return Ok(());
            }
        };

        let new_body = match edit_with_editor(terminal, &self.config.editor, &note.content) {
            Ok(body) => body,
            Err(err) => {
                self.set_message(format!("error: {err}"));
                return Ok(());
            }
        };

        if new_body == note.content {
            self.set_message("No changes made.".to_string());
            return Ok(());
        }

        let latest = self.storage.read(id)?;
        if latest.to_file_content() != note.to_file_content() {
            let copy_id = self.storage.next_id()?;
            let now = current_timestamp();
            self.storage.create(&Note {
                id: copy_id,
                title: format!("{} (sync conflict)", note.title),
                content: new_body,
                created_at: now.clone(),
                updated_at: now,
            })?;
            self.reload();
            self.request_sync();
            self.set_message(format!(
                "Note changed while editing; saved as note {copy_id}."
            ));
            return Ok(());
        }

        note.content = new_body;
        note.updated_at = current_timestamp();
        if let Err(err) = self.storage.update(&note) {
            self.set_message(format!("error: {err}"));
            return Ok(());
        }
        self.reload();
        self.request_sync();
        self.set_message(format!("Updated note {id}."));
        Ok(())
    }

    fn start_rename(&mut self, id: u64) {
        match self.notes.iter().find(|n| n.id == id) {
            Some(note) => {
                self.input = note.title.clone();
                self.input_purpose = InputPurpose::Rename(id);
                self.input_origin = self.mode;
                self.mode = Mode::TitleInput;
            }
            None => self.set_message(format!("Note {id} not found.")),
        }
    }

    fn cancel_input(&mut self) {
        self.input.clear();
        self.input_purpose = InputPurpose::NewNote;
        self.mode = self.input_origin;
    }

    fn commit_rename(&mut self, id: u64) -> io::Result<()> {
        let title = self.input.trim().to_string();
        self.mode = self.input_origin;

        if title.is_empty() {
            self.set_message("No title provided; title unchanged.".to_string());
            return Ok(());
        }

        let mut note = match self.storage.read(id) {
            Ok(note) => note,
            Err(err) => {
                self.set_message(format!("error: {err}"));
                return Ok(());
            }
        };

        if note.title == title {
            self.set_message("No changes made.".to_string());
            return Ok(());
        }

        note.title = title;
        note.updated_at = current_timestamp();
        if let Err(err) = self.storage.update(&note) {
            self.set_message(format!("error: {err}"));
            return Ok(());
        }
        self.reload();
        self.request_sync();
        self.set_message(format!("Renamed note {id}."));
        Ok(())
    }

    fn start_editor_setting(&mut self) {
        self.input = self.config.editor.clone();
        self.input_purpose = InputPurpose::EditorSetting;
        self.input_origin = Mode::Settings;
        self.mode = Mode::TitleInput;
    }

    fn commit_editor_setting(&mut self) -> io::Result<()> {
        let editor = self.input.trim().to_string();
        self.mode = Mode::Settings;

        if editor.is_empty() {
            self.set_message("No editor provided; editor unchanged.".to_string());
            return Ok(());
        }

        if self.config.editor == editor {
            self.set_message("No changes made.".to_string());
            return Ok(());
        }

        self.config.editor = editor;
        match self.config.save() {
            Ok(()) => self.set_message("Saved settings.".to_string()),
            Err(err) => self.set_message(format!("error: {err}")),
        }
        Ok(())
    }

    fn toggle_hide_extension(&mut self) {
        self.config.hide_extension = !self.config.hide_extension;
        match self.config.save() {
            Ok(()) => {
                let state = if self.config.hide_extension {
                    "on"
                } else {
                    "off"
                };
                self.set_message(format!("Hide extension {state}; settings saved."));
            }
            Err(err) => {
                self.config.hide_extension = !self.config.hide_extension;
                self.set_message(format!("error: {err}"));
            }
        }
    }

    fn handle_settings_key(&mut self, key: KeyEvent) -> io::Result<()> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.settings_row = self.settings_row.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.settings_row = (self.settings_row + 1).min(SettingsRow::COUNT - 1);
            }
            KeyCode::Enter | KeyCode::Char(' ') => match SettingsRow::from_index(self.settings_row)
            {
                SettingsRow::Editor => self.start_editor_setting(),
                SettingsRow::HideExtension => self.toggle_hide_extension(),
                SettingsRow::NotesDir => self.set_message(
                    "notes_dir is read-only here; use: console_note --set notes_dir <path>"
                        .to_string(),
                ),
                SettingsRow::Sync => self.request_sync(),
            },
            KeyCode::Esc | KeyCode::Backspace => self.mode = Mode::List,
            _ => {}
        }
        Ok(())
    }

    fn request_sync(&mut self) {
        if sync::SyncState::load(&self.config).ok().flatten().is_none() {
            self.set_message("Sync is not configured; use Settings > GitHub sync.".to_string());
        } else if let Some(sender) = &self.sync_request {
            let _ = sender.send(());
            self.set_message("Syncing...".to_string());
        }
    }

    fn configure_or_sync(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        if sync::SyncState::load(&self.config)?.is_some() {
            self.request_sync();
            return Ok(());
        }
        ratatui::restore();
        println!("GitHub sync setup. Register and install a GitHub App with device flow enabled.");
        print!("GitHub App client ID: ");
        io::stdout().flush()?;
        let mut client_id = String::new();
        io::stdin().read_line(&mut client_id)?;
        print!("Private repository (OWNER/REPO or new:REPO): ");
        io::stdout().flush()?;
        let mut target = String::new();
        io::stdin().read_line(&mut target)?;
        let outcome = sync::setup(&self.config, client_id.trim(), target.trim());
        *terminal = ratatui::init();
        match outcome {
            Ok(()) => self.request_sync(),
            Err(err) => self.set_message(format!("error: sync setup: {err}")),
        }
        Ok(())
    }

    fn create_note(&mut self, title: &str, terminal: &mut DefaultTerminal) -> io::Result<()> {
        let body = match edit_with_editor(terminal, &self.config.editor, "") {
            Ok(body) => body,
            Err(err) => {
                self.set_message(format!("error: {err}"));
                return Ok(());
            }
        };

        if body.trim().is_empty() {
            self.set_message("No content provided; note not created.".to_string());
            return Ok(());
        }

        let id = match self.storage.next_id() {
            Ok(id) => id,
            Err(err) => {
                self.set_message(format!("error: {err}"));
                return Ok(());
            }
        };
        let now = current_timestamp();
        let note = Note {
            id,
            title: title.to_string(),
            content: body,
            created_at: now.clone(),
            updated_at: now,
        };
        if let Err(err) = self.storage.create(&note) {
            self.set_message(format!("error: {err}"));
            return Ok(());
        }
        self.reload();
        self.set_message(format!("Created note {id}."));
        self.request_sync();
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        match self.mode {
            Mode::View => self.draw_view(frame),
            Mode::Settings => self.draw_settings(frame),
            Mode::TitleInput => {
                match self.input_origin {
                    Mode::View => self.draw_view(frame),
                    Mode::Settings => self.draw_settings(frame),
                    _ => self.draw_list(frame),
                }
                self.draw_title_input(frame);
            }
            Mode::ConfirmDelete => {
                self.draw_list(frame);
                self.draw_confirm(frame);
            }
            Mode::List => self.draw_list(frame),
        }
    }

    fn draw_list(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let [header, search, list, status, footer] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);

        let total = self.notes.len();
        let shown = self.filtered().len();
        let title = if self.filter.trim().is_empty() {
            format!(" console_note — {total} note(s) ")
        } else {
            format!(" console_note — {shown}/{total} note(s) ")
        };
        frame.render_widget(
            Block::default()
                .borders(Borders::BOTTOM)
                .title(Line::from(Span::styled(
                    title,
                    Style::default().add_modifier(Modifier::BOLD),
                )))
                .title_alignment(Alignment::Center),
            header,
        );

        let search_text = if self.filter_active {
            Line::from(vec![
                Span::styled("> ", Style::default().fg(Color::Yellow)),
                Span::raw(self.filter.clone()),
            ])
        } else {
            Line::from(vec![
                Span::styled("/", Style::default().fg(Color::DarkGray)),
                Span::styled(" to search", Style::default().fg(Color::DarkGray)),
            ])
        };
        frame.render_widget(Paragraph::new(search_text), search);

        let items: Vec<ListItem> = self
            .filtered()
            .iter()
            .map(|note| ListItem::new(self.list_line(note)))
            .collect();
        let list_widget = List::new(items)
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::White))
            .highlight_symbol("> ");
        frame.render_stateful_widget(list_widget, list, &mut self.list_state);
        if shown == 0 {
            let empty = Paragraph::new("No notes found.")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(empty, list);
        }

        let status_style = if self.status.starts_with("error") {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Cyan)
        };
        frame.render_widget(
            Paragraph::new(self.status.clone()).style(status_style),
            status,
        );

        let footer_text = " ↑/k ↓/j move  ↵ view  / search  n new  e edit  R rename  d delete  s settings  S sync  r refresh  q quit ";
        frame.render_widget(
            Paragraph::new(footer_text)
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center),
            footer,
        );
    }

    fn list_line(&self, note: &Note) -> Line<'static> {
        let title: String = note.title.chars().take(40).collect();
        let date = if note.created_at.len() >= 10 {
            &note.created_at[..10]
        } else {
            &note.created_at
        };
        let mut spans = vec![Span::styled(
            format!("{:>4} ", note.id),
            Style::default().fg(Color::DarkGray),
        )];
        if !self.config.hide_extension {
            spans.push(Span::styled(
                format!("{:<8}", format!("{}.md", note.id)),
                Style::default().fg(Color::DarkGray),
            ));
        }
        spans.push(Span::styled(
            title,
            Style::default().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            date.to_string(),
            Style::default().fg(Color::DarkGray),
        ));
        Line::from(spans)
    }

    fn draw_settings(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let [header, body, status, footer] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);

        frame.render_widget(
            Block::default()
                .borders(Borders::BOTTOM)
                .title(Line::from(Span::styled(
                    " console_note — settings ",
                    Style::default().add_modifier(Modifier::BOLD),
                )))
                .title_alignment(Alignment::Center),
            header,
        );

        let check = |on: bool| {
            if on {
                "[x]".to_string()
            } else {
                "[ ]".to_string()
            }
        };
        let rows = [
            (
                "Editor",
                self.config.editor.clone(),
                Some("Enter to edit".to_string()),
            ),
            (
                "Hide extension",
                check(self.config.hide_extension),
                Some("Enter to toggle".to_string()),
            ),
            (
                "Notes directory",
                self.config.notes_dir.display().to_string(),
                None,
            ),
            (
                "GitHub sync",
                sync::status(&self.config).unwrap_or_else(|_| "unavailable".to_string()),
                Some("Enter to set up or sync".to_string()),
            ),
        ];

        let selected = SettingsRow::from_index(self.settings_row);
        let lines: Vec<Line> = rows
            .iter()
            .enumerate()
            .map(|(i, (label, value, hint))| {
                let row = SettingsRow::from_index(i);
                let is_selected = row == selected;
                let marker = if is_selected { "> " } else { "  " };
                let label_style = if is_selected {
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                        .bg(Color::DarkGray)
                } else {
                    Style::default().add_modifier(Modifier::BOLD)
                };
                let value_style = if is_selected {
                    Style::default().fg(Color::White).bg(Color::DarkGray)
                } else if row == SettingsRow::NotesDir {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default().fg(Color::Cyan)
                };
                let mut spans = vec![
                    Span::styled(marker.to_string(), label_style),
                    Span::styled(format!("{:<16}", format!("{label}:")), label_style),
                    Span::styled(format!("{value} "), value_style),
                ];
                if let Some(hint) = hint {
                    let hint_style = if is_selected {
                        Style::default().fg(Color::Yellow).bg(Color::DarkGray)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };
                    spans.push(Span::styled(format!("({hint})"), hint_style));
                }
                Line::from(spans)
            })
            .collect();

        frame.render_widget(Paragraph::new(lines), body);

        let status_style = if self.status.starts_with("error") {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Cyan)
        };
        frame.render_widget(
            Paragraph::new(self.status.clone()).style(status_style),
            status,
        );

        let footer_text = " ↑/↓ j/k move  ↵ edit/toggle  Esc back ";
        frame.render_widget(
            Paragraph::new(footer_text)
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center),
            footer,
        );
    }

    fn draw_view(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let [header, body, footer] = Layout::vertical([
            Constraint::Length(4),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(area);

        let note = match self.notes.iter().find(|n| n.id == self.view_id) {
            Some(note) => note,
            None => {
                frame.render_widget(
                    Paragraph::new("Note not found.")
                        .alignment(Alignment::Center)
                        .style(Style::default().fg(Color::Red)),
                    area,
                );
                return;
            }
        };

        let header_text = vec![
            Line::from(Span::styled(
                format!(" Note #{} — {}", note.id, note.title),
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!(" created {}   updated {}", note.created_at, note.updated_at),
                Style::default().fg(Color::DarkGray),
            )),
        ];
        frame.render_widget(
            Paragraph::new(header_text).block(Block::default().borders(Borders::BOTTOM)),
            header,
        );

        let content_area = body;
        let content_height = content_area.height.saturating_sub(2).max(1) as usize;
        let max_scroll = self.content_line_count(note).saturating_sub(content_height);
        self.view_scroll = (self.view_scroll as usize).min(max_scroll) as u16;

        let paragraph = Paragraph::new(note.content.clone())
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: false })
            .scroll((self.view_scroll, 0));
        frame.render_widget(paragraph, content_area);

        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None);
        let mut scrollbar_state =
            ScrollbarState::new(self.content_line_count(note)).position(self.view_scroll as usize);
        frame.render_stateful_widget(scrollbar, content_area, &mut scrollbar_state);

        let footer_text = " ↑/↓ j/k scroll  PgUp/PgDn page  e edit  R rename  Esc back  q quit ";
        frame.render_widget(
            Paragraph::new(footer_text)
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center),
            footer,
        );
    }

    fn content_line_count(&self, note: &Note) -> usize {
        note.content.lines().count().max(1)
    }

    fn draw_title_input(&mut self, frame: &mut Frame) {
        let area = popup_rect(60, 3, frame.area());
        if area.width < 4 || area.height < 3 {
            return;
        }
        frame.render_widget(Clear, area);
        let title = match self.input_purpose {
            InputPurpose::Rename(_) => " Rename title ",
            InputPurpose::EditorSetting => " Editor command ",
            InputPurpose::NewNote => " New note title ",
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::Yellow));
        frame.render_widget(Paragraph::new(self.input.clone()).block(block), area);
        let inner_width = area.width.saturating_sub(2);
        let cursor_offset = (self.input.width() as u16).min(inner_width.saturating_sub(1));
        frame.set_cursor_position(Position::new(area.x + 1 + cursor_offset, area.y + 1));
    }

    fn draw_confirm(&mut self, frame: &mut Frame) {
        let area = popup_rect(50, 3, frame.area());
        if area.width < 4 || area.height < 3 {
            return;
        }
        frame.render_widget(Clear, area);
        let text = match self.confirm_id {
            Some(id) => format!("Delete note {id}? (y/N)"),
            None => String::new(),
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Confirm ")
            .border_style(Style::default().fg(Color::Yellow));
        frame.render_widget(
            Paragraph::new(text)
                .block(block)
                .alignment(Alignment::Center),
            area,
        );
    }
}

fn popup_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center);
    let [area] = vertical.areas(area);
    let horizontal = Layout::horizontal([Constraint::Percentage(percent_x)]).flex(Flex::Center);
    let [area] = horizontal.areas(area);
    area
}

fn edit_with_editor(
    terminal: &mut DefaultTerminal,
    editor_cmd: &str,
    initial: &str,
) -> io::Result<String> {
    ratatui::restore();
    let result = editor::edit_content(editor_cmd, initial);
    *terminal = ratatui::init();
    result
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::config::Config;
    use crate::storage::Storage;

    const STALE_MARKER: &str = "STALEMARKER";

    struct TestEnv {
        dir: PathBuf,
        app: App<'static>,
    }

    impl Drop for TestEnv {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn test_env(tag: &str, note_title: &str, note_id: u64) -> TestEnv {
        let dir = std::env::temp_dir().join(format!("console_note_tui_{tag}"));
        let _ = fs::remove_dir_all(&dir);
        let config = Config {
            editor: "true".to_string(),
            notes_dir: dir.join("notes"),
            config_dir: dir.clone(),
            hide_extension: true,
        };
        fs::create_dir_all(&config.notes_dir).unwrap();
        let storage = Storage::new(&config);
        if !note_title.is_empty() {
            storage
                .create(&Note {
                    id: note_id,
                    title: format!("{STALE_MARKER} {note_title}"),
                    content: String::new(),
                    created_at: "2026-01-01T00:00:00Z".to_string(),
                    updated_at: "2026-01-01T00:00:00Z".to_string(),
                })
                .unwrap();
        }
        let storage_ref: &'static Storage = Box::leak(Box::new(storage));
        let app = App::new(storage_ref, &config);
        TestEnv { dir, app }
    }

    fn draw(app: &mut App, width: u16, height: u16) -> Terminal<TestBackend> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        terminal
    }

    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        let buf = terminal.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn row_in(terminal: &Terminal<TestBackend>, rect: Rect, y: u16) -> String {
        let buf = terminal.backend().buffer();
        (rect.x..rect.right())
            .map(|x| buf[(x, y)].symbol())
            .collect::<String>()
    }

    #[test]
    fn confirm_dialog_renders_text_inside_borders() {
        for (width, height) in [(80, 24), (120, 30), (200, 50)] {
            let mut env = test_env("confirm", "delete me", 7);
            env.app.confirm_id = Some(7);
            env.app.mode = Mode::ConfirmDelete;
            let terminal = draw(&mut env.app, width, height);

            let text = buffer_text(&terminal);
            assert_eq!(
                text.matches("Delete note 7? (y/N)").count(),
                1,
                "confirm text must render exactly once at {width}x{height}"
            );

            let rect = popup_rect(50, 3, Rect::new(0, 0, width, height));
            assert_eq!(rect.height, 3);
            assert!(row_in(&terminal, rect, rect.y).contains("Confirm"));
            assert!(!row_in(&terminal, rect, rect.y + 1).contains(STALE_MARKER));
        }
    }

    #[test]
    fn title_input_dialog_shows_typed_text_without_bleedthrough() {
        for (width, height) in [(80, 24), (200, 50)] {
            let mut env = test_env("title_input", "old note", 1);
            env.app.input = "hello world".to_string();
            env.app.mode = Mode::TitleInput;
            let terminal = draw(&mut env.app, width, height);

            let rect = popup_rect(60, 3, Rect::new(0, 0, width, height));
            assert_eq!(rect.height, 3);
            assert!(row_in(&terminal, rect, rect.y).contains("New note title"));
            assert!(row_in(&terminal, rect, rect.y + 1).contains("hello world"));
            assert!(
                !row_in(&terminal, rect, rect.y + 1).contains(STALE_MARKER),
                "list content must not bleed through the dialog at {width}x{height}"
            );
        }
    }

    #[test]
    fn rename_dialog_renders_prefilled_text_without_bleedthrough() {
        for (width, height) in [(80, 24), (120, 30), (200, 50)] {
            let mut env = test_env("rename_dialog", "old note", 1);
            env.app.start_rename(1);
            env.app.input = "hello world".to_string();
            let terminal = draw(&mut env.app, width, height);

            let rect = popup_rect(60, 3, Rect::new(0, 0, width, height));
            assert_eq!(rect.height, 3);
            assert!(row_in(&terminal, rect, rect.y).contains("Rename title"));
            assert!(row_in(&terminal, rect, rect.y + 1).contains("hello world"));
            assert!(
                !row_in(&terminal, rect, rect.y + 1).contains(STALE_MARKER),
                "list content must not bleed through the dialog at {width}x{height}"
            );
        }
    }

    #[test]
    fn start_rename_prefills_input_with_current_title() {
        let mut env = test_env("rename_prefill", "original name", 4);
        env.app.mode = Mode::List;
        env.app.start_rename(4);
        assert_eq!(env.app.input_purpose, InputPurpose::Rename(4));
        assert_eq!(env.app.input_origin, Mode::List);
        assert_eq!(env.app.mode, Mode::TitleInput);
        assert_eq!(env.app.input, format!("{STALE_MARKER} original name"));
    }

    #[test]
    fn commit_rename_updates_title_and_timestamp_on_disk() {
        let mut env = test_env("rename_commit", "old name", 5);
        env.app.start_rename(5);
        env.app.input = "brand new name".to_string();

        env.app.commit_rename(5).unwrap();

        assert_eq!(env.app.mode, Mode::List);
        let note = env.app.storage.read(5).unwrap();
        assert_eq!(note.title, "brand new name");
        assert_eq!(note.created_at, "2026-01-01T00:00:00Z");
        assert_ne!(note.updated_at, "2026-01-01T00:00:00Z");
        assert!(env.app.notes.iter().any(|n| n.title == "brand new name"));
    }

    #[test]
    fn rename_from_view_returns_to_view_mode() {
        let mut env = test_env("rename_view", "viewed note", 9);
        env.app.view_id = 9;
        env.app.mode = Mode::View;
        env.app.start_rename(9);
        assert_eq!(env.app.input_origin, Mode::View);
        assert_eq!(env.app.mode, Mode::TitleInput);

        env.app.input = "renamed from view".to_string();
        env.app.commit_rename(9).unwrap();

        assert_eq!(env.app.mode, Mode::View);
        let note = env.app.storage.read(9).unwrap();
        assert_eq!(note.title, "renamed from view");
    }

    #[test]
    fn commit_rename_rejects_blank_title() {
        let mut env = test_env("rename_blank", "keep me", 2);
        env.app.start_rename(2);
        env.app.input = "   ".to_string();

        env.app.commit_rename(2).unwrap();

        assert_eq!(env.app.mode, Mode::List);
        let note = env.app.storage.read(2).unwrap();
        assert_eq!(note.title, format!("{STALE_MARKER} keep me"));
        assert_eq!(note.updated_at, "2026-01-01T00:00:00Z");
        assert_eq!(env.app.status, "No title provided; title unchanged.");
    }

    #[test]
    fn cancel_rename_restores_original_mode_and_discards_changes() {
        let mut env = test_env("rename_cancel", "untouched", 6);
        env.app.view_id = 6;
        env.app.mode = Mode::View;
        env.app.start_rename(6);
        assert_eq!(env.app.mode, Mode::TitleInput);

        env.app.cancel_input();

        assert_eq!(env.app.mode, Mode::View);
        assert_eq!(env.app.input_purpose, InputPurpose::NewNote);
        assert!(env.app.input.is_empty());
        let note = env.app.storage.read(6).unwrap();
        assert_eq!(note.title, format!("{STALE_MARKER} untouched"));
    }

    #[test]
    fn settings_page_renders_current_values() {
        for (width, height) in [(80, 24), (120, 30), (200, 50)] {
            let mut env = test_env("settings_render", "", 0);
            let notes_path = env.app.config.notes_dir.display().to_string();
            env.app.mode = Mode::Settings;
            let terminal = draw(&mut env.app, width, height);

            let text = buffer_text(&terminal);
            assert!(
                text.contains("console_note — settings"),
                "page title missing at {width}x{height}"
            );
            assert!(text.contains("Editor:"));
            assert!(text.contains("true"));
            assert!(text.contains("Hide extension:"));
            assert!(text.contains("[x]"), "default hide_extension must show [x]");
            assert!(text.contains("Notes directory:"));
            assert!(text.contains(&notes_path));
        }
    }

    #[test]
    fn toggle_hide_extension_flips_value_and_persists_to_disk() {
        let mut env = test_env("settings_toggle", "", 0);
        assert!(env.app.config.hide_extension);
        env.app.settings_row = 1;
        env.app.mode = Mode::Settings;

        env.app
            .handle_settings_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
            .unwrap();

        assert!(!env.app.config.hide_extension);
        assert_eq!(env.app.mode, Mode::Settings);
        let raw = fs::read_to_string(env.dir.join("config")).unwrap();
        assert!(raw.contains("hide_extension=false"));

        env.app
            .handle_settings_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
            .unwrap();
        assert!(env.app.config.hide_extension);
        let raw = fs::read_to_string(env.dir.join("config")).unwrap();
        assert!(raw.contains("hide_extension=true"));
    }

    #[test]
    fn editor_setting_commit_persists_to_disk() {
        let mut env = test_env("settings_editor", "", 0);
        env.app.mode = Mode::Settings;
        env.app.start_editor_setting();
        assert_eq!(env.app.input_purpose, InputPurpose::EditorSetting);
        assert_eq!(env.app.input_origin, Mode::Settings);
        assert_eq!(env.app.input, "true");

        env.app.input = "nano".to_string();
        env.app.commit_editor_setting().unwrap();

        assert_eq!(env.app.mode, Mode::Settings);
        assert_eq!(env.app.config.editor, "nano");
        let raw = fs::read_to_string(env.dir.join("config")).unwrap();
        assert!(raw.contains("editor=nano"));
    }

    #[test]
    fn editor_setting_rejects_blank_and_unchanged_input() {
        let mut env = test_env("settings_editor_blank", "", 0);
        env.app.start_editor_setting();

        env.app.input = "   ".to_string();
        env.app.commit_editor_setting().unwrap();
        assert_eq!(env.app.status, "No editor provided; editor unchanged.");
        assert_eq!(env.app.config.editor, "true");

        env.app.input = "true".to_string();
        env.app.commit_editor_setting().unwrap();
        assert_eq!(env.app.status, "No changes made.");
    }

    #[test]
    fn list_shows_filenames_only_when_extension_visible() {
        let mut env = test_env("filenames_hidden", "a note", 3);
        env.app.mode = Mode::List;
        let terminal = draw(&mut env.app, 80, 24);
        assert!(!buffer_text(&terminal).contains(".md"));

        env.app.config.hide_extension = false;
        let terminal = draw(&mut env.app, 80, 24);
        assert!(buffer_text(&terminal).contains("3.md"));
    }

    #[test]
    fn cursor_uses_display_width_not_byte_length() {
        let mut env = test_env("cursor_width", "", 0);
        env.app.input = "héllo→".to_string();
        env.app.mode = Mode::TitleInput;
        let terminal = draw(&mut env.app, 100, 30);

        let rect = popup_rect(60, 3, Rect::new(0, 0, 100, 30));
        let expected_x = rect.x + 1 + 6;
        assert_eq!(
            terminal.backend().cursor_position(),
            Position::new(expected_x, rect.y + 1)
        );
    }

    #[test]
    fn cursor_is_clamped_to_inner_width() {
        let mut env = test_env("cursor_clamp", "", 0);
        env.app.input = "x".repeat(100);
        env.app.mode = Mode::TitleInput;
        let terminal = draw(&mut env.app, 40, 12);

        let rect = popup_rect(60, 3, Rect::new(0, 0, 40, 12));
        let expected_x = rect.x + rect.width - 2;
        assert_eq!(
            terminal.backend().cursor_position(),
            Position::new(expected_x, rect.y + 1)
        );
    }

    #[test]
    fn dialogs_skip_rendering_on_tiny_terminals() {
        let mut env = test_env("tiny", "", 0);
        env.app.input = "abc".to_string();
        env.app.mode = Mode::TitleInput;
        let terminal = draw(&mut env.app, 20, 2);
        assert!(!buffer_text(&terminal).contains("New note title"));

        env.app.mode = Mode::ConfirmDelete;
        env.app.confirm_id = Some(7);
        let terminal = draw(&mut env.app, 20, 2);
        assert!(!buffer_text(&terminal).contains("Delete note 7"));
    }
}
