use clap::Parser;
use glasspane::{
    app::App,
    config::{Cli, Command, Config},
    cwd, git, ui,
};
use ratatui::{Terminal, backend::TestBackend};
use std::{fs, process::Command as Process};

#[test]
fn cli_accepts_paths_hooks_and_overrides() {
    let cli = Cli::try_parse_from([
        "glasspane",
        "/tmp/project",
        "--no-follow",
        "--commits",
        "12",
    ])
    .unwrap();
    assert_eq!(cli.path.unwrap().to_str(), Some("/tmp/project"));
    assert!(cli.no_follow);
    assert_eq!(cli.commits, Some(12));
    let cli = Cli::try_parse_from(["glasspane", "init", "zsh"]).unwrap();
    assert!(matches!(cli.command, Some(Command::Init { shell }) if shell == "zsh"));
    assert!(Cli::try_parse_from(["glasspane", "init", "fish"]).is_err());
}

#[test]
fn configuration_defaults_and_invalid_values() {
    let config: Config = toml::from_str("").unwrap();
    assert_eq!(config.commit_limit, 50);
    assert_eq!(config.graph_limit, 200);
    assert!(toml::from_str::<Config>("branch_sort = 'invalid'").is_err());
    assert!(toml::from_str::<Config>("typo = 123").is_err());
    assert!(toml::from_str::<Config>("commit_limit = -1").is_err());
    assert!(
        toml::to_string(&config)
            .unwrap()
            .contains("follow_cwd = true")
    );
}

#[test]
fn zsh_hook_creates_directory_and_tracks_spaces() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("project with spaces");
    fs::create_dir(&destination).unwrap();
    let hook = dir.path().join("hook.zsh");
    fs::write(&hook, cwd::ZSH_HOOK).unwrap();
    let output = Process::new("zsh")
        .args(["-dfc", "source \"$1\"; source \"$1\"; cd -- \"$2\""])
        .arg("glasspane-hook-test")
        .arg(&hook)
        .arg(&destination)
        .env("XDG_STATE_HOME", dir.path().join("state"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        cwd::read(&dir.path().join("state/glasspane/cwd")),
        Some(destination)
    );
}

#[test]
fn cwd_reader_preserves_trailing_spaces_and_rejects_relative_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("with trailing space ");
    fs::create_dir(&path).unwrap();
    let file = dir.path().join("cwd");
    fs::write(&file, format!("{}\n", path.display())).unwrap();
    assert_eq!(cwd::read(&file), Some(path));
    fs::write(&file, "relative\n").unwrap();
    assert!(cwd::read(&file).is_none());
}

#[test]
fn layouts_render_at_small_normal_and_wide_sizes_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    let mut app = App::new(dir.path().to_path_buf());
    app.no_color = true;
    app.apply(0, git::load(dir.path(), &Config::default()));
    for (width, height) in [(0, 0), (1, 1), (29, 11), (30, 12), (80, 24), (160, 48)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        for focus in 0..4 {
            app.focus = focus;
            app.scroll[focus] = usize::MAX;
            for help in [false, true] {
                app.help = help;
                terminal.draw(|frame| ui::render(frame, &mut app)).unwrap();
            }
        }
    }
}

#[test]
fn stale_repository_results_do_not_replace_new_directory() {
    let mut app = App::new("/old".into());
    app.change_cwd(Some("/new".into()));
    app.apply(0, Err(git::LoadError::NotRepository("/old".into())));
    assert!(app.error.is_none());
    assert!(app.loading);
    assert_eq!(app.cwd.to_str(), Some("/new"));
}

#[test]
fn terminal_content_does_not_emit_control_sequences() {
    assert_eq!(ui::safe("a\x1b[31m\nfile\t"), "a�[31m�file�");
}

#[test]
fn expanded_graph_uses_full_panel_and_restores_the_dashboard_grid() {
    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    let mut snapshot = git::load(dir.path(), &Config::default()).unwrap();
    snapshot.graph = Ok(git::graph::parse(include_str!("fixtures/graph.txt")));
    let mut app = App::new(dir.path().to_path_buf());
    app.apply(0, Ok(snapshot));
    let mut terminal = Terminal::new(TestBackend::new(140, 32)).unwrap();
    for expanded in [false, true, false] {
        app.maximized = expanded;
        terminal.draw(|frame| ui::render(frame, &mut app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Merge topic"));
        assert!(text.contains("[HEAD → main]"));
        assert_eq!(text.contains("Recent Commits"), !expanded);
        assert_eq!(text.contains("Staging"), !expanded);
        assert!(text.contains(if expanded {
            "Enter restore"
        } else {
            "Enter expand"
        }));
    }
}

#[test]
fn summary_cli_reports_a_repository_without_starting_a_tui() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    repo.set_head("refs/heads/main").unwrap();
    fs::write(dir.path().join("file.txt"), "staged\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("file.txt")).unwrap();
    index.write().unwrap();
    let output = Process::new(env!("CARGO_BIN_EXE_glasspane"))
        .arg("summary")
        .arg(dir.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Branch  main"));
    assert!(text.contains("No commits yet"));
    assert!(text.contains("1 staged · 0 unstaged · 0 untracked · 0 conflicted"));
    assert!(!text.contains('\x1b'));
    let not_repo = tempfile::tempdir().unwrap();
    let output = Process::new(env!("CARGO_BIN_EXE_glasspane"))
        .arg("summary")
        .arg(not_repo.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("Not a git repository")
    );
}

fn selectable_branch_app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    let mut snapshot = git::load(dir.path(), &Config::default()).unwrap();
    let branch = |name: String, current| git::refs::BranchInfo {
        name,
        current,
        hash: "1234567".into(),
        timestamp: 0,
    };
    snapshot.local_branches.push(branch("main".into(), true));
    for i in 0..30 {
        snapshot
            .local_branches
            .push(branch(format!("branch-{i:02}"), false));
    }
    snapshot.remote_branches = vec![
        branch("origin/first".into(), false),
        branch("origin/last".into(), false),
    ];
    let mut app = App::new(dir.path().into());
    app.apply(0, Ok(snapshot));
    app.focus = 1;
    (dir, app)
}

#[test]
fn branch_navigation_skips_group_headers_and_keeps_selection_visible() {
    let (_dir, mut app) = selectable_branch_app();
    assert_eq!(app.selected_branch.as_ref().unwrap().name, "main");
    app.go_edge(true);
    assert_eq!(app.selected_branch.as_ref().unwrap().name, "origin/last");
    let mut terminal = Terminal::new(TestBackend::new(140, 24)).unwrap();
    terminal.draw(|frame| ui::render(frame, &mut app)).unwrap();
    assert!(app.scroll[1] > 0);
    let buffer = terminal.backend().buffer();
    let text: String = buffer.content.iter().map(|c| c.symbol()).collect();
    assert!(text.contains("origin/last"));
    assert!(
        buffer
            .content
            .iter()
            .any(|c| c.symbol() == "›" && c.modifier.contains(ratatui::style::Modifier::REVERSED))
    );
    app.scroll_by(false);
    app.scroll_by(false);
    assert_eq!(app.selected_branch.as_ref().unwrap().name, "branch-29");
    assert!(!app.selected_branch.as_ref().unwrap().remote);
    app.go_edge(false);
    terminal.draw(|frame| ui::render(frame, &mut app)).unwrap();
    assert_eq!(app.scroll[1], 0);
}

#[test]
fn branch_selection_survives_reordering_and_falls_back_when_deleted() {
    let (_dir, mut app) = selectable_branch_app();
    for _ in 0..11 {
        app.scroll_by(true);
    }
    assert_eq!(app.selected_branch.as_ref().unwrap().name, "branch-10");
    let mut snapshot = app.snapshot.take().unwrap();
    snapshot.local_branches.swap(11, 21);
    app.apply(0, Ok(snapshot));
    assert_eq!(app.switch_request().unwrap().target.name, "branch-10");
    let mut snapshot = app.snapshot.take().unwrap();
    snapshot.local_branches.retain(|b| b.name != "branch-10");
    app.apply(0, Ok(snapshot));
    assert_eq!(app.selected_branch.as_ref().unwrap().name, "main");
}

#[test]
fn branch_switch_requests_are_blocked_while_busy_or_changing_repository() {
    let (_dir, mut app) = selectable_branch_app();
    assert!(app.switch_request().is_some());
    app.switching = true;
    assert!(app.switch_request().is_none());
    app.switching = false;
    let other = tempfile::tempdir().unwrap();
    app.change_cwd(Some(other.path().into()));
    assert!(
        app.switch_request().is_none(),
        "stale branch rows must not switch the previous repository"
    );
}

#[test]
fn amend_review_renders_modes_files_warning_and_small_terminal_guard() {
    use glasspane::actions::amend::{self, Mode, Review};
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    let mut index = repo.index().unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let signature = git2::Signature::now("Test", "test@example.invalid").unwrap();
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "Reviewed commit",
        &tree,
        &[],
    )
    .unwrap();
    for i in 0..20 {
        let name = format!("file-{i:02}.txt");
        fs::write(dir.path().join(&name), "staged\n").unwrap();
        index.add_path(std::path::Path::new(&name)).unwrap();
    }
    index.write().unwrap();
    let mut app = App::new(dir.path().into());
    app.apply(0, git::load(dir.path(), &Config::default()));
    app.amend = Some(Review::new(amend::prepare(dir.path()).unwrap()));
    let mut terminal = Terminal::new(TestBackend::new(90, 28)).unwrap();
    terminal.draw(|frame| ui::render(frame, &mut app)).unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(text.contains("Amend latest commit"));
    assert!(text.contains("Reviewed commit"));
    assert!(text.contains("Files included"));
    assert!(text.contains("Rewrites commit hash"));
    app.amend.as_mut().unwrap().choose(Mode::MessageOnly);
    app.amend.as_mut().unwrap().scroll_files(true, 28);
    terminal.draw(|frame| ui::render(frame, &mut app)).unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(text.contains("Files EXCLUDED; remain staged"));
    assert!(text.contains("file-11.txt"));
    let review = app.amend.as_mut().unwrap();
    review.file_scroll = 0;
    review.scroll_files(true, 18);
    assert_eq!(
        review.file_scroll, 1,
        "small terminals must not skip staged files when paging"
    );
    let mut terminal = Terminal::new(TestBackend::new(45, 16)).unwrap();
    terminal.draw(|frame| ui::render(frame, &mut app)).unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(text.contains("Enlarge terminal"));
    assert!(!text.contains("Enter: confirm"));
}

#[test]
fn full_and_short_commands_share_subcommands_and_branding() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    repo.set_head("refs/heads/main").unwrap();
    let mut summaries = vec![];
    for binary in [env!("CARGO_BIN_EXE_glasspane"), env!("CARGO_BIN_EXE_glass")] {
        let version = Process::new(binary).arg("--version").output().unwrap();
        assert!(version.status.success());
        assert!(String::from_utf8_lossy(&version.stdout).contains("glasspane 0.1.0"));
        let help = Process::new(binary).arg("--help").output().unwrap();
        assert!(help.status.success());
        assert!(String::from_utf8_lossy(&help.stdout).contains("summary"));
        let hook = Process::new(binary).args(["init", "zsh"]).output().unwrap();
        assert!(hook.status.success());
        assert_eq!(String::from_utf8(hook.stdout).unwrap(), cwd::ZSH_HOOK);
        let summary = Process::new(binary)
            .arg("summary")
            .arg(dir.path())
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(summary.status.success());
        assert!(String::from_utf8_lossy(&summary.stdout).contains("glasspane"));
        summaries.push(summary.stdout);
    }
    assert_eq!(summaries[0], summaries[1]);
}

#[test]
fn renamed_config_prefers_glasspane_and_falls_back_to_gitpane() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join("gitpane/config.toml");
    let current = dir.path().join("glasspane/config.toml");
    fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    fs::write(&legacy, "commit_limit = 13\n").unwrap();
    let read = || {
        Process::new(env!("CARGO_BIN_EXE_glass"))
            .arg("--print-config")
            .env("XDG_CONFIG_HOME", dir.path())
            .output()
            .unwrap()
    };
    let old = read();
    assert!(old.status.success());
    assert!(String::from_utf8_lossy(&old.stdout).contains("commit_limit = 13"));
    fs::create_dir_all(current.parent().unwrap()).unwrap();
    fs::write(&current, "commit_limit = 17\n").unwrap();
    let new = read();
    assert!(new.status.success());
    assert!(String::from_utf8_lossy(&new.stdout).contains("commit_limit = 17"));
    fs::write(&current, "unknown_setting = true\n").unwrap();
    assert!(
        !read().status.success(),
        "an invalid new config must not silently fall back"
    );
    assert_eq!(fs::read_to_string(&legacy).unwrap(), "commit_limit = 13\n");
}
