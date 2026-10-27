//! Interactive terminal view over the same linked Release Desk library.

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};
use release_desk_core::{Repository, Snapshot, latest_installer};
use release_desk_store::{Store, default_config_path};
use std::{
    error::Error,
    io::{self, IsTerminal},
    path::PathBuf,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Repositories,
    Installers,
    Settings,
}

struct App {
    project: String,
    account: String,
    snapshot: Snapshot,
    stale: bool,
    library: PathBuf,
    selected: usize,
    query: String,
    searching: bool,
    help: bool,
    view: View,
}

impl App {
    fn matching(&self) -> Vec<&Repository> {
        let needle = self.query.to_lowercase();
        self.snapshot
            .repos
            .iter()
            .filter(|repo| {
                needle.is_empty()
                    || repo.name.to_lowercase().contains(&needle)
                    || repo
                        .description
                        .as_deref()
                        .is_some_and(|description| description.to_lowercase().contains(&needle))
            })
            .collect()
    }

    fn key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
            return true;
        }
        if self.searching {
            match code {
                KeyCode::Esc | KeyCode::Enter => self.searching = false,
                KeyCode::Backspace => {
                    self.query.pop();
                    self.selected = 0;
                }
                KeyCode::Char(c) => {
                    self.query.push(c);
                    self.selected = 0;
                }
                _ => {}
            }
            return false;
        }
        if self.help {
            self.help = false;
            return false;
        }
        match code {
            KeyCode::Char('q') => return true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('/') => self.searching = true,
            KeyCode::Char('1') => self.view = View::Repositories,
            KeyCode::Char('2') => self.view = View::Installers,
            KeyCode::Char('3') => self.view = View::Settings,
            KeyCode::Down | KeyCode::Char('j') => {
                let len = self.matching().len();
                if len > 0 {
                    self.selected = (self.selected + 1).min(len - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            _ => {}
        }
        false
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let mut config = default_config_path()?;
    let mut project = "ArtCraft".to_owned();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => config = PathBuf::from(args.next().ok_or("--config needs a path")?),
            "--project" => project = args.next().ok_or("--project needs a name")?,
            "--help" | "-h" => {
                println!(
                    "release-desk-tui [--config PATH] [--project NAME]\nKeys: 1 repositories, 2 installers, 3 settings, / search, j/k move, ? help, q quit"
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("Release Desk TUI requires an interactive terminal".into());
    }
    let store = Store::open_at(&config)?;
    let project = store.project(&project)?;
    let view = store.cached_snapshot(&project)?;
    let mut app = App {
        project: project.name,
        account: project.account,
        snapshot: view.snapshot,
        stale: view.stale,
        library: store.library_root().to_path_buf(),
        selected: 0,
        query: String::new(),
        searching: false,
        help: false,
        view: View::Repositories,
    };
    run(&mut app)
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        if let Err(error) = execute!(io::stdout(), EnterAlternateScreen) {
            disable_raw_mode()?;
            return Err(error);
        }
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

fn run(app: &mut App) -> Result<(), Box<dyn Error>> {
    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    loop {
        terminal.draw(|frame| draw(frame, app))?;
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press && app.key(key.code, key.modifiers) {
                break;
            }
        }
    }
    Ok(())
}

fn draw(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(area);
    let title = format!(
        " RELEASE DESK  /  {}  ·  {}{} ",
        app.project,
        app.account,
        if app.stale { "  ·  CACHED" } else { "" }
    );
    frame.render_widget(
        Paragraph::new(title)
            .block(Block::default().borders(Borders::ALL))
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        layout[0],
    );
    let titles = ["1 Repositories", "2 Installers", "3 Settings"];
    let index = match app.view {
        View::Repositories => 0,
        View::Installers => 1,
        View::Settings => 2,
    };
    frame.render_widget(
        Tabs::new(titles)
            .select(index)
            .block(Block::default().borders(Borders::ALL))
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        layout[1],
    );
    if app.help {
        frame.render_widget(Paragraph::new("Keyboard\n\n1/2/3 switch views  ·  j/k or arrows move  ·  / search\n? toggle help  ·  q quit  ·  Ctrl+C quit")
            .block(Block::default().title("Help").borders(Borders::ALL)).wrap(Wrap { trim: true }), layout[2]);
    } else {
        match app.view {
            View::Repositories | View::Installers => draw_repositories(frame, app, layout[2]),
            View::Settings => {
                frame.render_widget(Paragraph::new(format!("Library: {}\nSnapshot generated: {}\nRepositories: {}\nSource: linked read-only", app.library.display(), app.snapshot.generated, app.snapshot.repos.len()))
                    .block(Block::default().title("Library settings").borders(Borders::ALL)).wrap(Wrap { trim: true }), layout[2]);
            }
        }
    }
    let footer = if app.searching {
        format!("Search: {}_  (Enter to finish)", app.query)
    } else {
        format!(
            "/ search  ·  j/k navigate  ·  ? help  ·  q quit    {} matches",
            app.matching().len()
        )
    };
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(Color::Gray)),
        layout[3],
    );
}

fn draw_repositories(frame: &mut Frame<'_>, app: &App, area: ratatui::layout::Rect) {
    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(36), Constraint::Percentage(64)])
        .split(area);
    let repos = app.matching();
    let items: Vec<ListItem<'_>> = repos
        .iter()
        .map(|repo| {
            let marker = if latest_installer(repo, false, "dmg").is_some() {
                "●"
            } else {
                " "
            };
            ListItem::new(format!("{} {}", marker, repo.name))
        })
        .collect();
    let mut state = ListState::default();
    if !items.is_empty() {
        state.select(Some(app.selected.min(items.len() - 1)));
    }
    frame.render_stateful_widget(
        List::new(items)
            .block(Block::default().title("Repositories").borders(Borders::ALL))
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        panels[0],
        &mut state,
    );
    let detail = repos.get(app.selected).map(|repo| {
        if app.view == View::Installers {
            match latest_installer(repo, false, "dmg") {
                Some((release, assets)) => format!("{}\nLatest installer: {}\n\n{}", repo.name, release.tag_name,
                    assets.iter().map(|asset| format!("{}  ·  {} bytes", asset.name, asset.size)).collect::<Vec<_>>().join("\n")),
                None => format!("{}\nNo eligible DMG release", repo.name),
            }
        } else {
            format!("{}\n\n{}\n\nLanguage: {}\nArchived: {}\nReleases: {}\nDefault branch: {}\n\n{}",
                repo.name, repo.description.as_deref().unwrap_or("No description"),
                repo.primary_language.as_ref().map_or("Unknown", |language| &language.name),
                if repo.is_archived { "Yes" } else { "No" }, repo.releases.as_ref().map_or(0, |r| r.nodes.len()),
                repo.default_branch_ref.as_ref().map_or("Unknown", |branch| &branch.name), repo.url)
        }
    }).unwrap_or_else(|| "No repositories match this search.".to_owned());
    frame.render_widget(
        Paragraph::new(detail)
            .block(Block::default().title("Details").borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        panels[1],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_navigation_and_search_are_bounded() {
        let snapshot =
            release_desk_core::parse_snapshot(r#"{"org":"owner","generated":"now","repos":[]}"#)
                .expect("fixture");
        let mut app = App {
            project: "Test".into(),
            account: "owner".into(),
            snapshot,
            stale: false,
            library: PathBuf::new(),
            selected: 0,
            query: String::new(),
            searching: false,
            help: false,
            view: View::Repositories,
        };
        assert!(!app.key(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.selected, 0);
        app.key(KeyCode::Char('/'), KeyModifiers::NONE);
        app.key(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(app.query, "a");
        app.key(KeyCode::Esc, KeyModifiers::NONE);
        app.key(KeyCode::Char('2'), KeyModifiers::NONE);
        assert!(app.view == View::Installers);
        assert!(app.key(KeyCode::Char('q'), KeyModifiers::NONE));
    }
}
