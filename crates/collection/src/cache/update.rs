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

    fn finish(self) {
        let inner = self.inner;
        self.pending
            .into_iter()
            .for_each(|(a, _)| inner.proceed_update(a))
    }

    pub(super) fn run(mut self) {
        loop {
            match self.queue.recv_timeout(self.interval) {
                Ok(Some(action)) => {
                    self.pending.insert(action, SystemTime::now());
                }
                Ok(None) => {
                    self.finish();
                    return;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    error!("OngoingUpdater channel disconnected preliminary");
                    self.finish();
                    return;
                }
                Err(RecvTimeoutError::Timeout) => (), // just give chance to send pending actions
            }
            let current_time = SystemTime::now();
            // TODO replace with drain_filter when becomes stable
            let mut ready = self
                .pending
                .iter()
                .filter(|(_, time)| current_time.duration_since(**time).unwrap() > self.interval)
                .map(|v| (v.1.clone(), v.0.clone()))
                .collect::<Vec<_>>();
            ready.sort_unstable_by_key(|i| i.0);
            ready.into_iter().for_each(|(_, a)| {
                self.pending.remove(&a);
                self.inner.proceed_update(a)
            })
        }
    }
}

pub(super) struct RecursiveUpdater<'a> {
    queue: VecDeque<AudioFolderShort>,
    inner: &'a CacheInner,
}

impl<'a> RecursiveUpdater<'a> {
    pub(super) fn new(inner: &'a CacheInner, root: Option<AudioFolderShort>) -> Self {
        let root = root.unwrap_or_else(|| AudioFolderShort {
            name: "root".into(),
            path: Path::new("").into(),
            is_file: false,
            modified: None,
        });
        let mut queue = VecDeque::new();
        queue.push_back(root);
        RecursiveUpdater { queue, inner }
    }

    pub(super) fn process(mut self) {
        while let Some(folder_info) = self.queue.pop_front() {
            // process AF
            let full_path = self.inner.base_dir().join(&folder_info.path);
            let mod_ts = get_modified(full_path);
            let af = match self.inner.get_if_actual(&folder_info.path, mod_ts) {
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
