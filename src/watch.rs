//! Notices changes in `todo/` made outside markoff (another editor, git, the shell).

use std::{
    path::Path,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

use color_eyre::{Result, eyre::WrapErr};
use notify::{
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{AccessKind, AccessMode},
};

/// Quiet time after the last change before reloading. Saving a file often causes several
/// events (temp file, rename, chmod), this turns them into one reload.
const DEBOUNCE: Duration = Duration::from_millis(100);

pub struct TodoWatcher {
    /// Only kept alive, watching stops when it is dropped.
    _watcher: RecommendedWatcher,
    events: Receiver<notify::Result<Event>>,
    debounce: Debounce,
}

impl TodoWatcher {
    pub fn new(dir: &Path) -> Result<Self> {
        let (sender, events) = mpsc::channel();
        let mut watcher =
            notify::recommended_watcher(sender).wrap_err("failed to start watcher")?;
        watcher
            .watch(dir, RecursiveMode::Recursive)
            .wrap_err_with(|| format!("failed to watch {}", dir.display()))?;
        Ok(Self {
            _watcher: watcher,
            events,
            debounce: Debounce::default(),
        })
    }

    /// Takes all waiting events without blocking. Returns `true` once a reload is due.
    pub fn poll(&mut self) -> Result<bool> {
        let now = Instant::now();
        while let Ok(event) = self.events.try_recv() {
            if is_change(&event.wrap_err("file watcher failed")?) {
                self.debounce.touch(now);
            }
        }
        Ok(self.debounce.ready(now))
    }
}

/// Everything except reads. Reloading reads all files, so counting reads as changes would
/// make markoff reload itself forever. Closing a file after writing is a change.
fn is_change(event: &Event) -> bool {
    match event.kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        _ => true,
    }
}

/// Fires once no change happened for [`DEBOUNCE`].
#[derive(Debug, Default)]
struct Debounce {
    last_change: Option<Instant>,
}

impl Debounce {
    fn touch(&mut self, now: Instant) {
        self.last_change = Some(now);
    }

    fn ready(&mut self, now: Instant) -> bool {
        match self.last_change {
            Some(last) if now.duration_since(last) >= DEBOUNCE => {
                self.last_change = None;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, thread};

    use notify::event::{CreateKind, ModifyKind};
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn debounce_waits_for_quiet_and_fires_once() {
        let start = Instant::now();
        let mut debounce = Debounce::default();
        assert!(!debounce.ready(start), "nothing happened");

        debounce.touch(start);
        debounce.touch(start + DEBOUNCE / 2);
        assert!(
            !debounce.ready(start + DEBOUNCE),
            "still within the quiet time"
        );
        assert!(debounce.ready(start + DEBOUNCE * 2));
        assert!(!debounce.ready(start + DEBOUNCE * 3), "fires only once");
    }

    #[test]
    fn reads_are_not_changes() {
        let event = |kind| Event::new(kind);
        assert!(is_change(&event(EventKind::Create(CreateKind::File))));
        assert!(is_change(&event(EventKind::Modify(ModifyKind::Any))));
        assert!(is_change(&event(EventKind::Access(AccessKind::Close(
            AccessMode::Write
        )))));
        assert!(!is_change(&event(EventKind::Access(AccessKind::Close(
            AccessMode::Read
        )))));
        assert!(!is_change(&event(EventKind::Access(AccessKind::Open(
            AccessMode::Any
        )))));
    }

    #[test]
    fn notices_a_new_file() {
        let dir = TempDir::new().unwrap();
        let mut watcher = TodoWatcher::new(dir.path()).unwrap();
        fs::write(dir.path().join("new.md"), "# New").unwrap();

        // Real file system events take a moment, poll until the debounce fires.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !watcher.poll().unwrap() {
            assert!(Instant::now() < deadline, "no reload within 5s");
            thread::sleep(Duration::from_millis(20));
        }
    }
}
