use crate::{
    actions::{
        Actions,
        amend::{self, Mode, Review},
    },
    app::{App, Loader},
    config::{Cli, Command, Config},
    cwd,
    event::{Event, WatchCommand},
    shell, summary, ui, watch,
};
use anyhow::{Context, Result};
use clap::Parser;
use crossterm::{
    cursor::Show,
    event::{self, Event as TerminalEvent, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{
    io::{self, IsTerminal},
    sync::mpsc,
    time::{Duration, Instant},
};

struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}
fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
}

pub fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Some(Command::Init { .. }) => {
            print!("{}", cwd::ZSH_HOOK);
            return Ok(());
        }
        Some(Command::Summary { path }) => {
            let path = path.clone().unwrap_or(std::env::current_dir()?);
            summary::print(&path)?;
            return Ok(());
        }
        None => {}
    }
    let config = Config::load(&cli)?;
    if cli.print_config {
        print!("{}", toml::to_string_pretty(&config)?);
        return Ok(());
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        anyhow::bail!(
            "glass needs an interactive terminal. Use --help or --print-config for noninteractive output."
        );
    }
    let launch = cli.path.clone().unwrap_or(std::env::current_dir()?);
    let launch = if launch.is_absolute() {
        launch
    } else {
        std::env::current_dir()?.join(launch)
    };
    let state_file = config.follow_cwd.then(cwd::state_file).flatten();
    let mut app = App::new(launch);
    // Saved hook state may belong to another shell or an earlier session.
    // Always start at the launch directory; the watcher follows later changes.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous_hook(info);
    }));
    run(&mut app, config, state_file)?;
    summary::print(&app.cwd)?;
    Ok(())
}

fn run(app: &mut App, config: Config, state_file: Option<std::path::PathBuf>) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    let (watch_tx, watch_rx) = mpsc::channel();
    let watcher = watch::spawn(
        tx.clone(),
        watch_rx,
        state_file,
        Duration::from_millis(config.debounce_ms),
    );
    let diff_worker = crate::git::diff::Worker::spawn(tx.clone());
    let actions = Actions::spawn(tx.clone());
    let loader = Loader::spawn(config, tx, watch_tx.clone());
    loader.request(app.cwd.clone(), app.generation);
    let outcome = (|| -> Result<()> {
        enable_raw_mode().context("Cannot enable terminal raw mode")?;
        // This scope restores the terminal before waiting for worker shutdown.
        let _guard = TerminalGuard;
        execute!(io::stdout(), EnterAlternateScreen)?;
        let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        if cfg!(debug_assertions) && std::env::var_os("GLASSPANE_DEBUG_PANIC").is_some() {
            panic!("requested terminal restoration test");
        }
        let mut redraw = true;
        let mut last_draw = Instant::now();
        loop {
            if loader.is_finished()
                || watcher.is_finished()
                || actions.is_finished()
                || diff_worker.is_finished()
            {
                anyhow::bail!("A background worker stopped unexpectedly; restart glass.");
            }
            for event in rx.try_iter() {
                match event {
                    Event::InspectionLoaded { id, result } => app.apply_inspection(id, result),
                    Event::DiffLoaded { id, result } => app.apply_diff(id, result),
                    Event::Loaded { generation, result } => app.apply(generation, *result),
                    Event::CommitPrepared { generation, result } => {
                        app.commit_preparing = false;
                        app.action_message = None;
                        if generation == app.generation {
                            match *result {
                                Ok(plan) => {
                                    app.diff = None;
                                    app.inspection = None;
                                    app.help = false;
                                    app.commit = Some(crate::actions::commit::Review::new(plan));
                                }
                                Err(error) => {
                                    app.action_error = Some(error);
                                    app.help = true;
                                }
                            }
                        }
                    }
                    Event::AmendPrepared { generation, result } => {
                        app.amend_preparing = false;
                        app.action_message = None;
                        if generation == app.generation {
                            match *result {
                                Ok(plan) => {
                                    app.diff = None;
                                    app.inspection = None;
                                    app.help = false;
                                    app.amend = Some(Review::new(plan));
                                }
                                Err(error) => {
                                    app.action_error = Some(error);
                                    app.help = true;
                                }
                            }
                        }
                    }
                    Event::StagingChanged { generation, result } => {
                        app.staging_busy = false;
                        if generation == app.generation {
                            match result {
                                Ok(message) => {
                                    app.action_message = Some(message);
                                    app.action_error = None;
                                }
                                Err(error) => {
                                    app.action_error = Some(error);
                                    app.action_message = None;
                                    app.help = true;
                                }
                            }
                        }
                        app.loading = true;
                        app.refresh_diff(&diff_worker);
                        loader.request(app.cwd.clone(), app.generation);
                    }
                    Event::BranchSwitched { generation, result } => {
                        app.switching = false;
                        if generation == app.generation {
                            match result {
                                Ok(message) => {
                                    app.action_message = Some(message);
                                    app.action_error = None;
                                }
                                Err(error) => {
                                    app.action_error = Some(error);
                                    app.help = true;
                                    app.action_message = None;
                                }
                            }
                        } else {
                            app.action_message = None;
                        }
                        app.loading = true;
                        loader.request(app.cwd.clone(), app.generation);
                    }
                    Event::RepoChanged => {
                        app.refresh_diff(&diff_worker);
                        app.loading = true;
                        loader.request(app.cwd.clone(), app.generation);
                    }
                    Event::CwdChanged(path) => {
                        app.change_cwd(path);
                        loader.request(app.cwd.clone(), app.generation);
                    }
                    Event::WatchStatus { polling, error } => {
                        app.polling = polling;
                        app.watcher_error = error;
                    }
                }
                redraw = true;
            }
            if redraw || last_draw.elapsed() >= Duration::from_secs(30) {
                terminal.draw(|frame| ui::render(frame, app))?;
                redraw = false;
                last_draw = Instant::now();
            }
            if event::poll(Duration::from_millis(250))? {
                match event::read()? {
                    TerminalEvent::Key(key) if key.kind != KeyEventKind::Release => {
                        if app.commit.is_some() {
                            let cancel = key.code == KeyCode::Esc
                                || (key.code == KeyCode::Char('c')
                                    && key.modifiers.contains(KeyModifiers::CONTROL));
                            if cancel {
                                app.commit = None;
                                redraw = true;
                                continue;
                            }
                            if key.code == KeyCode::Char('s')
                                && key.modifiers.contains(KeyModifiers::CONTROL)
                                && key.kind == KeyEventKind::Press
                            {
                                let size = terminal.size()?;
                                if size.width >= 60 && size.height >= 20 {
                                    let mut review = app.commit.take().unwrap();
                                    let outcome = shell::with_terminal(&mut terminal, || {
                                        crate::actions::commit::execute(
                                            &review.plan,
                                            &review.editor.text,
                                        )
                                    })?;
                                    match outcome {
                                        Ok(message) => {
                                            app.action_message = Some(message);
                                            app.action_error = None;
                                        }
                                        Err(error) => {
                                            review.error = Some(format!("{error:#}"));
                                            app.commit = Some(review);
                                        }
                                    }
                                    app.loading = true;
                                    loader.request(app.cwd.clone(), app.generation);
                                }
                            } else {
                                let review = app.commit.as_mut().unwrap();
                                let editor = &mut review.editor;
                                match key.code {
                                    KeyCode::Char(ch)
                                        if !key.modifiers.intersects(
                                            KeyModifiers::CONTROL | KeyModifiers::ALT,
                                        ) =>
                                    {
                                        editor.insert(&ch.to_string())
                                    }
                                    KeyCode::Enter => editor.insert("\n"),
                                    KeyCode::Backspace => editor.backspace(),
                                    KeyCode::Delete => editor.delete(),
                                    KeyCode::Left => editor.left(),
                                    KeyCode::Right => editor.right(),
                                    KeyCode::Home => editor.home(),
                                    KeyCode::End => editor.end(),
                                    KeyCode::Up => editor.vertical(false),
                                    KeyCode::Down => editor.vertical(true),
                                    KeyCode::PageDown => {
                                        review.file_scroll = review
                                            .file_scroll
                                            .saturating_add(5)
                                            .min(review.plan.staged.len().saturating_sub(1))
                                    }
                                    KeyCode::PageUp => {
                                        review.file_scroll = review.file_scroll.saturating_sub(5)
                                    }
                                    _ => {}
                                }
                            }
                            redraw = true;
                            continue;
                        }
                        if let Some(view) = &mut app.diff {
                            match key.code {
                                KeyCode::Esc | KeyCode::Char('q') => app.diff = None,
                                KeyCode::Char('c')
                                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    app.diff = None
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    view.scroll = view.scroll.saturating_add(1).min(view.max_scroll)
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    view.scroll = view.scroll.saturating_sub(1)
                                }
                                KeyCode::PageDown => {
                                    view.scroll = view
                                        .scroll
                                        .saturating_add(
                                            terminal.size()?.height.saturating_sub(4) as usize
                                        )
                                        .min(view.max_scroll)
                                }
                                KeyCode::PageUp => {
                                    view.scroll = view.scroll.saturating_sub(
                                        terminal.size()?.height.saturating_sub(4) as usize,
                                    )
                                }
                                KeyCode::Home | KeyCode::Char('g') => view.scroll = 0,
                                KeyCode::End | KeyCode::Char('G') => view.scroll = view.max_scroll,
                                KeyCode::Right | KeyCode::Char('l') => {
                                    view.horizontal = view.horizontal.saturating_add(4)
                                }
                                KeyCode::Left | KeyCode::Char('h') => {
                                    view.horizontal = view.horizontal.saturating_sub(4)
                                }
                                KeyCode::Char('r') => app.refresh_diff(&diff_worker),
                                _ => {}
                            }
                            redraw = true;
                            continue;
                        }
                        if let Some(view) = &mut app.inspection {
                            match key.code {
                                KeyCode::Esc | KeyCode::Char('q') => app.inspection = None,
                                KeyCode::Char('c')
                                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    app.inspection = None
                                }
                                KeyCode::Tab | KeyCode::BackTab => {
                                    view.message_focus = !view.message_focus
                                }
                                KeyCode::Down | KeyCode::Char('j') => view.navigate(true, 1),
                                KeyCode::Up | KeyCode::Char('k') => view.navigate(false, 1),
                                KeyCode::PageDown => view.navigate(true, 5),
                                KeyCode::PageUp => view.navigate(false, 5),
                                KeyCode::Char('g') | KeyCode::Home => {
                                    view.navigate(false, usize::MAX)
                                }
                                KeyCode::Char('G') | KeyCode::End => {
                                    view.navigate(true, usize::MAX)
                                }
                                KeyCode::Enter => app.open_commit_diff(&diff_worker),
                                _ => {}
                            }
                            redraw = true;
                            continue;
                        }
                        if app.amend.is_some() {
                            let cancel = key.code == KeyCode::Esc
                                || key.code == KeyCode::Char('q')
                                || (key.code == KeyCode::Char('c')
                                    && key.modifiers.contains(KeyModifiers::CONTROL));
                            if cancel {
                                app.amend = None;
                                redraw = true;
                                continue;
                            }
                            let review = app.amend.as_mut().unwrap();
                            match key.code {
                                KeyCode::Char('1') => review.choose(Mode::StagedKeepMessage),
                                KeyCode::Char('2') => review.choose(Mode::MessageOnly),
                                KeyCode::Char('3') => review.choose(Mode::StagedEditMessage),
                                KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => {
                                    review.step(true)
                                }
                                KeyCode::Char('k') | KeyCode::Up | KeyCode::BackTab => {
                                    review.step(false)
                                }
                                KeyCode::PageDown => {
                                    review.scroll_files(true, terminal.size()?.height)
                                }
                                KeyCode::PageUp => {
                                    review.scroll_files(false, terminal.size()?.height)
                                }
                                KeyCode::Enter if key.kind == KeyEventKind::Press => {
                                    let size = terminal.size()?;
                                    if size.width >= 60 && size.height >= 18 {
                                        let review = app.amend.take().unwrap();
                                        let outcome = shell::with_terminal(&mut terminal, || {
                                            amend::execute(&review.plan, review.mode)
                                        })?;
                                        match outcome {
                                            Ok(message) => {
                                                app.action_message = Some(message);
                                                app.action_error = None;
                                            }
                                            Err(error) => {
                                                app.action_error = Some(format!("{error:#}"));
                                                app.help = true;
                                            }
                                        }
                                        app.loading = true;
                                        loader.request(app.cwd.clone(), app.generation);
                                    }
                                }
                                _ => {}
                            }
                            redraw = true;
                            continue;
                        }
                        if key.code == KeyCode::Char('q')
                            || (key.code == KeyCode::Char('c')
                                && key.modifiers.contains(KeyModifiers::CONTROL))
                        {
                            break;
                        }
                        if !app.switching
                            && !app.amend_preparing
                            && !app.staging_busy
                            && !app.commit_preparing
                        {
                            app.action_message = None;
                        }
                        match key.code {
                            KeyCode::Char('s') | KeyCode::Char('u')
                                if !app.help
                                    && app.focus == 3
                                    && key.kind == KeyEventKind::Press =>
                            {
                                let operation = if key.code == KeyCode::Char('s') {
                                    crate::actions::staging::Operation::Stage
                                } else {
                                    crate::actions::staging::Operation::Unstage
                                };
                                if let Some(request) = app.staging_request(operation) {
                                    match actions.stage(request) {
                                        Ok(()) => {
                                            app.staging_busy = true;
                                            app.action_message = Some("Updating staging…".into());
                                        }
                                        Err(error) => {
                                            app.action_error = Some(error.to_string());
                                            app.help = true;
                                        }
                                    }
                                }
                            }
                            KeyCode::Char('c')
                                if !app.help
                                    && !app.switching
                                    && !app.staging_busy
                                    && !app.amend_preparing
                                    && !app.commit_preparing
                                    && key.kind == KeyEventKind::Press =>
                            {
                                if app.snapshot_generation == Some(app.generation)
                                    && !app.loading
                                    && let Some(snapshot) = &app.snapshot
                                {
                                    match actions
                                        .prepare_commit(snapshot.root.clone(), app.generation)
                                    {
                                        Ok(()) => {
                                            app.commit_preparing = true;
                                            app.action_message =
                                                Some("Preparing commit review…".into());
                                            app.action_error = None;
                                        }
                                        Err(error) => {
                                            app.action_error = Some(error.to_string());
                                            app.help = true;
                                        }
                                    }
                                }
                            }
                            KeyCode::Char('?') => app.help = !app.help,
                            KeyCode::Char('a')
                                if !app.help
                                    && !app.switching
                                    && !app.staging_busy
                                    && !app.amend_preparing
                                    && !app.commit_preparing
                                    && key.kind == KeyEventKind::Press =>
                            {
                                if app.snapshot_generation == Some(app.generation)
                                    && let Some(snapshot) = &app.snapshot
                                {
                                    app.shell_error = None;
                                    app.action_error = None;
                                    match actions
                                        .prepare_amend(snapshot.root.clone(), app.generation)
                                    {
                                        Ok(()) => {
                                            app.amend_preparing = true;
                                            app.action_message =
                                                Some("Preparing amend review…".into());
                                        }
                                        Err(error) => {
                                            app.action_error = Some(error.to_string());
                                            app.help = true;
                                        }
                                    }
                                }
                            }
                            KeyCode::Esc => {
                                app.action_error = None;
                                if app.help {
                                    app.help = false;
                                } else {
                                    app.maximized = false;
                                }
                            }
                            KeyCode::Enter if !app.help && app.focus == 2 => {
                                app.open_inspection(&diff_worker);
                            }
                            KeyCode::Enter if !app.help && app.focus == 3 => {
                                app.open_diff(&diff_worker);
                            }
                            KeyCode::Enter if !app.help && app.focus == 1 => {
                                if key.kind == KeyEventKind::Press
                                    && let Some(request) = app.switch_request()
                                {
                                    app.shell_error = None;
                                    app.action_message =
                                        Some(format!("Switching to {}…", request.target.name));
                                    match actions.request(request) {
                                        Ok(()) => {
                                            app.switching = true;
                                            app.action_error = None;
                                        }
                                        Err(error) => {
                                            app.action_error = Some(error.to_string());
                                            app.help = true;
                                            app.action_message = None;
                                        }
                                    }
                                }
                            }
                            KeyCode::Enter | KeyCode::Char(' ') if !app.help => {
                                app.maximized = !app.maximized
                            }
                            KeyCode::Char('!')
                                if !app.help
                                    && !app.switching
                                    && !app.amend_preparing
                                    && !app.commit_preparing
                                    && !app.staging_busy =>
                            {
                                app.action_error = None;
                                app.shell_error = shell::open(&mut terminal, &app.cwd)?;
                                if app.shell_error.is_some() {
                                    app.help = true;
                                }
                                app.loading = true;
                                loader.request(app.cwd.clone(), app.generation);
                            }
                            KeyCode::Tab if !app.help => app.focus = (app.focus + 1) % 4,
                            KeyCode::BackTab if !app.help => app.focus = (app.focus + 3) % 4,
                            KeyCode::Char('j') | KeyCode::Down if !app.help => app.scroll_by(true),
                            KeyCode::Char('k') | KeyCode::Up if !app.help => app.scroll_by(false),
                            KeyCode::Char('g') if !app.help => app.go_edge(false),
                            KeyCode::Char('G') if !app.help => app.go_edge(true),
                            KeyCode::Char('r') => {
                                app.loading = true;
                                loader.request(app.cwd.clone(), app.generation);
                            }
                            _ => {}
                        }
                        redraw = true;
                    }
                    TerminalEvent::Resize(_, _) => redraw = true,
                    _ => {}
                }
            }
        }
        Ok(())
    })();
    drop(actions);
    drop(loader);
    let _ = watch_tx.send(WatchCommand::Stop);
    let _ = watcher.join();
    outcome
}
