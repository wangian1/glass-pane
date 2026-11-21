use glasspane::{
    app::Loader,
    config::Config,
    event::{Event, WatchCommand},
    watch::{self, WatchTarget},
};
use std::{
    fs,
    sync::mpsc,
    time::{Duration, Instant},
};

#[test]
fn live_refresh_detects_worktree_edits_with_native_or_polling_backend() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    let (events, rx) = mpsc::channel();
    let (tx, commands) = mpsc::channel();
    let handle = watch::spawn(events, commands, None, Duration::from_millis(50));
    tx.send(WatchCommand::Target(Some(WatchTarget {
        root: repo.workdir().unwrap().into(),
        git_dir: repo.path().into(),
        common_dir: repo.path().into(),
    })))
    .unwrap();
    let ready = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    match ready {
        Event::WatchStatus {
            polling: false,
            error: None,
        } => {}
        Event::WatchStatus {
            polling: true,
            error: Some(error),
        } => {
            eprintln!("Native watcher unavailable; verifying polling fallback: {error}");
        }
        other => panic!("Expected watcher readiness, got {other:?}"),
    }
    // Target installation may follow the initial no-repository watcher setup.
    while let Ok(event) = rx.recv_timeout(Duration::from_millis(200)) {
        if let Event::WatchStatus {
            polling: true,
            error,
        } = event
        {
            assert!(
                error.is_some(),
                "polling must explain why native watching failed"
            );
        }
    }
    fs::write(repo.workdir().unwrap().join("new.txt"), "hello").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut detected = false;
    while Instant::now() < deadline {
        if matches!(
            rx.recv_timeout(Duration::from_millis(250)),
            Ok(Event::RepoChanged)
        ) {
            detected = true;
            break;
        }
    }
    tx.send(WatchCommand::Stop).unwrap();
    handle.join().unwrap();
    assert!(detected, "no refresh after a worktree edit");
}

#[test]
fn background_loader_coalesces_requests_and_finishes_on_latest_directory() {
    let first = tempfile::tempdir().unwrap();
    let latest = tempfile::tempdir().unwrap();
    git2::Repository::init(first.path()).unwrap();
    git2::Repository::init(latest.path()).unwrap();
    let (tx, rx) = mpsc::channel();
    let (watch_tx, _watch_rx) = mpsc::channel();
    let loader = Loader::spawn(Config::default(), tx, watch_tx);
    for generation in 0..100 {
        loader.request(first.path().into(), generation);
    }
    loader.request(latest.path().into(), 100);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut found = false;
    while Instant::now() < deadline {
        if let Ok(Event::Loaded {
            generation: 100,
            result,
        }) = rx.recv_timeout(Duration::from_millis(250))
        {
            assert_eq!(
                result
                    .as_ref()
                    .as_ref()
                    .unwrap()
                    .root
                    .canonicalize()
                    .unwrap(),
                latest.path().canonicalize().unwrap()
            );
            found = true;
            break;
        }
    }
    drop(loader);
    assert!(found, "latest refresh was lost");
}
