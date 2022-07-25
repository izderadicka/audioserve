use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use crossbeam_channel::{Receiver, RecvTimeoutError};
use notify::DebouncedEvent;

use crate::{util::get_modified, AudioFolderShort};

use super::CacheInner;

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub(crate) enum UpdateAction {
    RefreshFolder(PathBuf),
    RefreshFolderRecursive(PathBuf),
    RemoveFolder(PathBuf),
    RenameFolder { from: PathBuf, to: PathBuf },
}

impl UpdateAction {
    fn is_covered_by(&self, other: &UpdateAction) -> bool {
        match self {
            UpdateAction::RefreshFolder(my_path) => match other {
                UpdateAction::RefreshFolder(other_path) => my_path == other_path,
                UpdateAction::RefreshFolderRecursive(other_path) => my_path.starts_with(other_path),
                UpdateAction::RemoveFolder(other_path) => my_path.starts_with(other_path),
                UpdateAction::RenameFolder { .. } => false,
            },
            UpdateAction::RefreshFolderRecursive(my_path) => match other {
                UpdateAction::RefreshFolder(_) => false,
                UpdateAction::RefreshFolderRecursive(other_path) => my_path.starts_with(other_path),
                UpdateAction::RemoveFolder(other_path) => my_path.starts_with(other_path),
                UpdateAction::RenameFolder { .. } => false,
            },
            UpdateAction::RemoveFolder(my_path) => match other {
                UpdateAction::RefreshFolder(_) => false,
                UpdateAction::RefreshFolderRecursive(_) => false,
                UpdateAction::RemoveFolder(other_path) => my_path.starts_with(other_path),
                UpdateAction::RenameFolder { .. } => false,
            },
            UpdateAction::RenameFolder {
                from: my_from,
                to: my_to,
            } => match other {
                UpdateAction::RefreshFolder(_) => false,
                UpdateAction::RefreshFolderRecursive(_) => false,
                UpdateAction::RemoveFolder(_) => false,
                UpdateAction::RenameFolder {
                    from: other_from,
                    to: other_to,
                } => my_from == other_from && my_to == other_to,
            },
        }
    }
}

impl AsRef<Path> for UpdateAction {
    fn as_ref(&self) -> &Path {
        match self {
            UpdateAction::RefreshFolder(folder) => folder.as_path(),
            UpdateAction::RefreshFolderRecursive(folder) => folder.as_path(),
            UpdateAction::RemoveFolder(folder) => folder.as_path(),
            UpdateAction::RenameFolder { from, .. } => from.as_path(),
        }
    }
}

pub(super) struct OngoingUpdater {
    queue: Receiver<Option<UpdateAction>>,
    inner: Arc<CacheInner>,
    pending: HashMap<UpdateAction, SystemTime>,
    interval: Duration,
}

impl OngoingUpdater {
    pub(super) fn new(queue: Receiver<Option<UpdateAction>>, inner: Arc<CacheInner>) -> Self {
        OngoingUpdater {
            queue,
            inner,
            pending: HashMap::new(),
            interval: Duration::from_secs(10),
        }
    }

    fn send_actions(&mut self) {
        if self.pending.len() > 0 {
            // TODO: Would replacing map with empty be more efficient then iterating?
            let done = std::mem::replace(&mut self.pending, HashMap::new());
            let mut ready = done.into_iter().collect::<Vec<_>>();
            ready.sort_unstable_by(|a, b| b.1.cmp(&a.1));

            while let Some((a, _)) = ready.pop() {
                if !ready
                    .iter()
                    .skip(ready.len().saturating_sub(100)) // This is speculative optimalization, not be O(n^2) for large sets, but work for decent changes
                    .any(|(other, _)| a.is_covered_by(other))
                {
                    self.inner.proceed_update(a.clone())
                }
            }
        }
    }

    pub(super) fn run(mut self) {
        loop {
            match self.queue.recv_timeout(self.interval) {
                Ok(Some(action)) => {
                    self.pending.insert(action, SystemTime::now());
                    if self.pending.len() >= 10_000 {
                        // should not grow too big
                        self.send_actions();
                    }
                }
                Ok(None) => {
                    self.send_actions();
                    return;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    error!("OngoingUpdater channel disconnected preliminary");
                    self.send_actions();
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {
                    //every pending action is older then limit
                    self.send_actions();
                }
            }
        }
    }
}

pub(super) struct RecursiveUpdater<'a> {
    queue: VecDeque<AudioFolderShort>,
    inner: &'a CacheInner,
    force_update: bool,
}

impl<'a> RecursiveUpdater<'a> {
    pub(super) fn new(
        inner: &'a CacheInner,
        root: Option<AudioFolderShort>,
        force_update: bool,
    ) -> Self {
        let root = root.unwrap_or_else(|| AudioFolderShort {
            name: "root".into(),
            path: Path::new("").into(),
            is_file: false,
            modified: None,
            finished: false,
        });
        let mut queue = VecDeque::new();
        queue.push_back(root);
        RecursiveUpdater {
            queue,
            inner,
            force_update,
        }
    }

    pub(super) fn process(mut self) {
        while let Some(folder_info) = self.queue.pop_front() {
            // process AF
            let full_path = self.inner.base_dir().join(&folder_info.path);
            let mod_ts = get_modified(full_path);
            let af = match if self.force_update {
                None
            } else {
                self.inner.get_if_actual(&folder_info.path, mod_ts)
            } {
                None => match self.inner.force_update(&folder_info.path, true) {
                    Ok(af) => {
                        af.unwrap() // safe to unwrap as we set ret param
                    }
                    Err(e) => {
                        error!(
                            "Cannot update audio folder {:?}, error {}",
                            folder_info.path, e
                        );
                        continue;
                    }
                },
                Some(af) => {
                    debug!("For path {:?} using cached data", folder_info.path);
                    af
                }
            };
            self.queue.extend(af.subfolders)
        }
    }
}

pub(crate) enum FilteredEvent {
    Pass(DebouncedEvent),
    Error(notify::Error, Option<PathBuf>),
    Rescan,
    Ignore,
}

pub(crate) fn filter_event(evt: DebouncedEvent) -> FilteredEvent {
    use FilteredEvent::*;
    match evt {
        DebouncedEvent::NoticeWrite(_) => Ignore,
        DebouncedEvent::NoticeRemove(_) => Ignore,
        evt @ DebouncedEvent::Create(_) => Pass(evt),
        evt @ DebouncedEvent::Write(_) => Pass(evt),
        DebouncedEvent::Chmod(_) => Ignore,
        evt @ DebouncedEvent::Remove(_) => Pass(evt),
        evt @ DebouncedEvent::Rename(_, _) => Pass(evt),
        DebouncedEvent::Rescan => Rescan,
        DebouncedEvent::Error(e, p) => Error(e, p),
    }
}
