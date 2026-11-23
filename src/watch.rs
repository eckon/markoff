//! Notices changes in the root made outside markoff (another editor, git, the shell).

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
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

pub struct RootWatcher {
    /// Only kept alive, watching stops when it is dropped.
    _watcher: RecommendedWatcher,
    events: Receiver<notify::Result<Event>>,
    debounce: Debounce,
    /// The root as given and as the system resolves it, events may come with either (macOS
    /// reports `/private/var` for `/var`).
    roots: [PathBuf; 2],
    /// What changed since the last reload, relative to the root.
    changed: BTreeSet<PathBuf>,
}

impl RootWatcher {
    pub fn new(dir: &Path) -> Result<Self> {
        let (sender, events) = mpsc::channel();
        let mut watcher =
            notify::recommended_watcher(sender).wrap_err("failed to start watcher")?;
        watcher
            .watch(dir, RecursiveMode::Recursive)
            .wrap_err_with(|| format!("failed to watch {}", dir.display()))?;
        let resolved = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        Ok(Self {
            _watcher: watcher,
            events,
            debounce: Debounce::default(),
            roots: [dir.to_path_buf(), resolved],
            changed: BTreeSet::new(),
        })
    }

    /// Takes all waiting events without blocking. Returns what changed, relative to the root,
    /// once a reload is due. An empty path is the root: the system lost track of the changes
    /// (an event without paths), or one came from outside it.
    pub fn poll(&mut self) -> Result<Option<BTreeSet<PathBuf>>> {
        let now = Instant::now();
        while let Ok(event) = self.events.try_recv() {
            let event = event.wrap_err("file watcher failed")?;
            if !is_change(&event) || !self.debounce.touch(now) {
                continue;
            }
            if event.paths.is_empty() {
                self.changed.insert(PathBuf::new());
            }
            for path in &event.paths {
                let relative = (self.roots.iter()).find_map(|root| path.strip_prefix(root).ok());
                self.changed
                    .insert(relative.map_or_else(PathBuf::new, Path::to_path_buf));
            }
        }
        Ok(self
            .debounce
            .ready(now)
            .then(|| std::mem::take(&mut self.changed)))
    }

    /// Drops the changes a reload has just read. Our own writes reach the watcher only after
    /// the reload that follows them, without this every action would be read twice.
    pub fn skip(&mut self) {
        self.debounce.skip(Instant::now());
        self.changed.clear();
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
    /// Changes before this are dropped, see [`Self::skip`].
    skip_until: Option<Instant>,
}

impl Debounce {
    /// Returns whether the change counts, see [`Self::skip`].
    fn touch(&mut self, now: Instant) -> bool {
        let counts = self.skip_until.is_none_or(|until| now >= until);
        if counts {
            self.last_change = Some(now);
        }
        counts
    }

    /// Forgets the changes so far, and those of the next [`DEBOUNCE`]: events take a moment
    /// to arrive. A change from outside in that time is missed until the next one.
    fn skip(&mut self, now: Instant) {
        self.last_change = None;
        self.skip_until = Some(now + DEBOUNCE);
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
    fn skip_drops_changes_until_the_debounce_passed() {
        let start = Instant::now();
        let mut debounce = Debounce::default();
        debounce.touch(start);
        debounce.skip(start);
        debounce.touch(start + DEBOUNCE / 2);
        assert!(!debounce.ready(start + DEBOUNCE * 3), "all changes dropped");

        debounce.touch(start + DEBOUNCE);
        assert!(debounce.ready(start + DEBOUNCE * 2), "later changes count");
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
        let mut watcher = RootWatcher::new(dir.path()).unwrap();
        fs::write(dir.path().join("new.md"), "# New").unwrap();

        // Real file system events take a moment, poll until the debounce fires.
        let deadline = Instant::now() + Duration::from_secs(5);
        let changed = loop {
            if let Some(changed) = watcher.poll().unwrap() {
                break changed;
            }
            assert!(Instant::now() < deadline, "no reload within 5s");
            thread::sleep(Duration::from_millis(20));
        };
        assert!(changed.contains(Path::new("new.md")), "{changed:?}");
    }

    #[test]
    fn skip_drops_the_events_of_a_write_just_made() {
        let dir = TempDir::new().unwrap();
        let mut watcher = RootWatcher::new(dir.path()).unwrap();
        fs::write(dir.path().join("new.md"), "# New").unwrap();
        watcher.skip();

        let end = Instant::now() + DEBOUNCE * 3;
        while Instant::now() < end {
            assert_eq!(watcher.poll().unwrap(), None, "own write reloaded");
            thread::sleep(Duration::from_millis(20));
        }
    }
}
