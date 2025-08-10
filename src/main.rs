use std::{fs, io, path::PathBuf};
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};
use ignore::gitignore::GitignoreBuilder;

#[derive(Clone)]
struct Entry {
    name: String,
    path: PathBuf,
    hidden: bool,
    ignored: bool,
    selected: bool,
}

struct App {
    items: Vec<Entry>,
    cursor: usize,
    preview_content: String,
    show_preview: bool,
}

impl App {
    fn new(mut items: Vec<Entry>) -> Self {
        items.sort_by(|a, b| {
            match (a.hidden, b.hidden) {
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            }
        });
        let mut app = Self { items, cursor: 0, preview_content: String::new(), show_preview: true };
        app.update_preview();
        app
    }
    fn select_all(&mut self) {
        for it in &mut self.items { it.selected = true; }
    }
    fn select_none(&mut self) {
        for it in &mut self.items { it.selected = false; }
    }
    fn select_only_n(&mut self, n: usize) {
        self.select_none();
        if !self.items.is_empty() {
            let idx = n.min(self.items.len() - 1);
            self.items[idx].selected = true;
            self.cursor = idx;
        }
    }
    fn toggle_current(&mut self) {
        if self.items.is_empty() { return; }
        let it = &mut self.items[self.cursor];
        it.selected = !it.selected;
    }
    fn move_up(&mut self) {
        if self.items.is_empty() { return; }
        if self.cursor == 0 { self.cursor = self.items.len() - 1; } else { self.cursor -= 1; }
        self.update_preview();
    }
    fn move_down(&mut self) {
        if self.items.is_empty() { return; }
        self.cursor = (self.cursor + 1) % self.items.len();
        self.update_preview();
    }
    fn selected_paths(&self) -> Vec<PathBuf> {
        self.items.iter().filter(|e| e.selected).map(|e| e.path.clone()).collect()
    }
    fn selected_count(&self) -> usize {
        self.items.iter().filter(|e| e.selected).count()
    }
    
    fn update_preview(&mut self) {
        if self.items.is_empty() {
            self.preview_content = "No files available".to_string();
            return;
        }
        
        let current_file = &self.items[self.cursor].path;
        self.preview_content = match fs::read_to_string(current_file) {
            Ok(content) => {
                if content.is_empty() {
                    "<empty file>".to_string()
                } else if content.len() > 10000 {
                    format!("{}

... (truncated, file is {} bytes)", &content[..10000], content.len())
                } else {
                    content
                }
            }
            Err(e) => format!("Error reading file: {}", e),
        };
    }
    
    fn toggle_preview(&mut self) {
        self.show_preview = !self.show_preview;
    }
}

fn build_ignore_matcher() -> ignore::gitignore::Gitignore {
    let mut builder = GitignoreBuilder::new(".");
    let _ = builder.add(".gitignore");
    builder.build().unwrap_or_else(|_| ignore::gitignore::Gitignore::empty())
}

fn list_files() -> io::Result<Vec<Entry>> {
    let gi = build_ignore_matcher();
    let mut out = Vec::new();
    for ent in fs::read_dir(".")? {
        let ent = ent?;
        let path = ent.path();
        if !path.is_file() { continue; }
        let name = match path.file_name().and_then(|s| s.to_str()) { Some(s) => s.to_string(), None => continue };
        let hidden = name.starts_with('.');
        let ignored = gi.matched_path_or_any_parents(&path, false).is_ignore();
        out.push(Entry { name, path, hidden, ignored, selected: false });
    }
    Ok(out)
}

fn draw(ui: &mut Frame, app: &App, list_state: &mut ListState) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(5)].as_ref())
        .split(ui.size());

    let content_area = if app.show_preview {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)].as_ref())
            .split(main_chunks[0])
            .to_vec()
    } else {
        vec![main_chunks[0]]
    };

    let items: Vec<ListItem> = app.items.iter().enumerate().map(|(_i, e)| {
        let mark = if e.selected { "✓" } else { " " };
        let line = format!(" [{}] {}", mark, e.name);
        let style = if e.hidden || e.ignored {
            Style::default().fg(Color::Gray).add_modifier(Modifier::DIM)
        } else {
            Style::default().fg(Color::White)
        };
        ListItem::new(line).style(style)
    }).collect();

    let list = List::new(items)
        .block(Block::default().title("sharkit").borders(Borders::ALL))
        .highlight_style(Style::default().bg(Color::Cyan).fg(Color::Black))
        .highlight_symbol("› ");

    ui.render_stateful_widget(list, content_area[0], list_state);

    if app.show_preview && content_area.len() > 1 {
        let preview_title = if app.items.is_empty() {
            "Preview".to_string()
        } else {
            format!("Preview: {}", app.items[app.cursor].name)
        };

        let preview = Paragraph::new(app.preview_content.as_str())
            .block(Block::default().title(preview_title).borders(Borders::ALL))
            .wrap(Wrap { trim: false })
            .scroll((0, 0));

        ui.render_widget(preview, content_area[1]);
    }

    let help_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)].as_ref())
        .split(main_chunks[1]);

    let navigation_help = Paragraph::new("Navigation:\n[↑/↓ or j/k] move cursor\n[space] toggle selection\n[enter] confirm  [q/esc] quit")
        .block(Block::default().title("Controls").borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    ui.render_widget(navigation_help, help_chunks[0]);

    let selection_help = Paragraph::new(format!("Selection:\n[a/A] select all\n[n] select none\n[p] toggle preview\n\n{} selected", app.selected_count()))
        .block(Block::default().title("Actions").borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    ui.render_widget(selection_help, help_chunks[1]);
}

fn main() -> Result<()> {
    let items = list_files()?;
    let mut app = App::new(items);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut list_state = ListState::default();
    if !app.items.is_empty() { list_state.select(Some(0)); }

    let mut confirmed = false;
    loop {
        terminal.draw(|f| draw(f, &app, &mut list_state))?;

        if let Event::Key(KeyEvent { code, modifiers, .. }) = event::read()? {
            match (code, modifiers) {
                (KeyCode::Up, _) | (KeyCode::Char('k'), _) => { app.move_up(); list_state.select(Some(app.cursor)); }
                (KeyCode::Down, _) | (KeyCode::Char('j'), _) => { app.move_down(); list_state.select(Some(app.cursor)); }
                (KeyCode::Char(' '), _) => app.toggle_current(),
                (KeyCode::Char('a'), _) | (KeyCode::Char('A'), _) => app.select_all(),
                (KeyCode::Char('n'), _) => app.select_none(),
                (KeyCode::Enter, _) => { confirmed = true; break; }
                (KeyCode::Esc, _) | (KeyCode::Char('q'), _) => { confirmed = false; break; }
                (KeyCode::Char('1'), KeyModifiers::SHIFT) => app.select_only_n(0),
                (KeyCode::Char('2'), KeyModifiers::SHIFT) => app.select_only_n(1),
                (KeyCode::Char('3'), KeyModifiers::SHIFT) => app.select_only_n(2),
                (KeyCode::Char('4'), KeyModifiers::SHIFT) => app.select_only_n(3),
                (KeyCode::Char('5'), KeyModifiers::SHIFT) => app.select_only_n(4),
                (KeyCode::Char('6'), KeyModifiers::SHIFT) => app.select_only_n(5),
                (KeyCode::Char('7'), KeyModifiers::SHIFT) => app.select_only_n(6),
                (KeyCode::Char('8'), KeyModifiers::SHIFT) => app.select_only_n(7),
                (KeyCode::Char('9'), KeyModifiers::SHIFT) => app.select_only_n(8),
                (KeyCode::Char('0'), KeyModifiers::SHIFT) => {
                    if !app.items.is_empty() { app.select_only_n(app.items.len() - 1); }
                }
                (KeyCode::Char('p'), _) => app.toggle_preview(),
                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, LeaveAlternateScreen)?;

    if confirmed {
        let sel = app.selected_paths();
        for p in sel {
            println!("{}", pathdiff::diff_paths(&p, ".").unwrap_or(p).display());
        }
        std::process::exit(0);
    } else {
        std::process::exit(130);
    }
}
