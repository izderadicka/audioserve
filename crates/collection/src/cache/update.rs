use std::{
    cmp::Ordering,
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use indexmap::IndexMap;
use notify::{
    event::{ModifyKind, RemoveKind, RenameMode},
    Event, EventKind,
};

use crate::{util::get_modified, AudioFolderShort};

use super::{util::parent_path, CacheInner};

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub(crate) struct UpdateAction {
    pub path: PathBuf,
    pub kind: UpdateActionKind,
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub(crate) enum UpdateActionKind {
    RefreshFolder,
    RefreshFolderRecursive,
    RemoveFolder,
    RenameFolder { to: PathBuf },
}

impl UpdateAction {
    pub fn new(path: impl Into<PathBuf>, kind: UpdateActionKind) -> Self {
        UpdateAction {
            path: path.into(),
            kind,
        }
    }

    /// Assuming other is on same or parent folder !!!
    fn is_covered_by(&self, other: &UpdateActionKind, other_is_parent: bool) -> bool {
        match &self.kind {
            UpdateActionKind::RefreshFolder => match other {
                UpdateActionKind::RefreshFolder => !other_is_parent,
                UpdateActionKind::RefreshFolderRecursive => true,
                UpdateActionKind::RemoveFolder => true,
                UpdateActionKind::RenameFolder { .. } => false,
            },
            UpdateActionKind::RefreshFolderRecursive => match other {
                UpdateActionKind::RefreshFolder => false,
                UpdateActionKind::RefreshFolderRecursive => true,
                UpdateActionKind::RemoveFolder => true,
                UpdateActionKind::RenameFolder { .. } => false,
            },
            UpdateActionKind::RemoveFolder => match other {
                UpdateActionKind::RefreshFolder => false,
                UpdateActionKind::RefreshFolderRecursive => false,
                UpdateActionKind::RemoveFolder => true,
                UpdateActionKind::RenameFolder { .. } => false,
            },
            UpdateActionKind::RenameFolder { .. } => false,
        }
    }
}

impl AsRef<Path> for UpdateAction {
    fn as_ref(&self) -> &Path {
        self.path.as_path()
    }
}

#[derive(Debug)]
pub enum Modification {
    Created,
    Deleted,
    Modified,
    MovedTo(PathBuf),
}

impl Modification {
    fn from_event_kind(kind: EventKind, other_path: Option<PathBuf>) -> Option<Self> {
        match kind {
            EventKind::Create(_) => Some(Self::Created),
            EventKind::Modify(ModifyKind::Name(RenameMode::From)) => Some(Self::Deleted),
            EventKind::Modify(ModifyKind::Name(RenameMode::To)) => Some(Self::Created),
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
                other_path.map(|path| Self::MovedTo(path))
            }
            EventKind::Modify(_) => Some(Modification::Modified),
            EventKind::Remove(_) => Some(Self::Deleted),
            _ => None,
        }
    }
}
#[derive(Debug)]
struct PendingEvent {
    last_change: Instant,
    change_type: Modification,
}

pub(super) struct OngoingUpdater {
    input_channel: Receiver<Option<Event>>,
    inner: Arc<CacheInner>,
    pending: HashMap<PathBuf, PendingEvent>,
    interval: Duration,
    update_sender: Sender<Option<UpdateAction>>,
}

impl OngoingUpdater {
    pub(super) fn new(
        channel: Receiver<Option<Event>>,
        update_sender: Sender<Option<UpdateAction>>,
        inner: Arc<CacheInner>,
        debounce_interval: u32,
    ) -> Self {
        OngoingUpdater {
            input_channel: channel,
            inner,
            pending: HashMap::new(),
            interval: Duration::from_secs(debounce_interval as u64),
            update_sender,
        }
    }

    fn send_update_actions(&mut self, all: bool) {
        if !self.pending.is_empty() {
            let capacity = self.pending.capacity();
            let done = std::mem::take(&mut self.pending).into_iter();
            let mut done = if all {
                done.collect::<Vec<_>>()
            } else {
                let now = Instant::now();
                let mut expired = Vec::with_capacity(capacity);
                for (path, evt) in done {
                    if now.duration_since(evt.last_change) < self.interval {
                        self.pending.insert(path, evt);
                    } else {
                        expired.push((path, evt))
                    }
                }
                expired
            };

            done.sort_unstable_by(|(path_a, evt_a), (path_b, evt_b)| {
                let comparison = path_a.cmp(path_b);

                if let Ordering::Equal = comparison {
                    evt_a.last_change.cmp(&evt_b.last_change)
                } else {
                    comparison
                }
            });

            let mut actions: IndexMap<PathBuf, UpdateActionKind> = IndexMap::new();

            for (path, evt) in done.into_iter() {
                trace!("Notify debounced event: {:?} {:?}", path, evt);
                let event_actions = self.list_actions_for_event(&path, evt);
                for action in event_actions.into_iter() {
                    let parent = action.path.parent();

                    if let Some(existing_action) = parent.and_then(|path| actions.get(path)) {
                        if action.is_covered_by(existing_action, true) {
                            continue;
                        }
                    }

                    if let Some(existing_action) = actions.get(&action.path) {
                        if action.is_covered_by(existing_action, false) {
                            continue;
                        }
                    }

                    actions.insert(action.path, action.kind);
                }
            }

            for (path, kind) in actions.into_iter() {
                let action = UpdateAction::new(path, kind);
                self.update_sender
                    .send(Some(action))
                    .unwrap_or_else(|_| error!("Update receiver removed early"));
            }
        }
    }

    fn list_actions_for_event(&self, path: &Path, evt: PendingEvent) -> Vec<UpdateAction> {
        let mut result = Vec::new();
        let col_path = self.inner.strip_base(&path);

        match evt.change_type {
            Modification::Created => {
                if self.inner.path_type(path).is_dir() {
                    result.push(UpdateAction::new(
                        col_path,
                        UpdateActionKind::RefreshFolderRecursive,
                    ));
                }
                result.push(UpdateAction::new(
                    self.inner.get_true_parent(col_path, path),
                    UpdateActionKind::RefreshFolder,
                ));
            }
            Modification::Modified => {
                // TODO: check logic
                if self.inner.path_type(path).is_dir() {
                    // should be single file folder
                    result.push(UpdateAction::new(col_path, UpdateActionKind::RefreshFolder));
                } else {
                    result.push(UpdateAction::new(
                        self.inner.get_true_parent(col_path, path),
                        UpdateActionKind::RefreshFolder,
                    ));
                }
            }
            Modification::Deleted => {
                if self.inner.path_type(path).is_dir() {
                    result.push(UpdateAction::new(col_path, UpdateActionKind::RemoveFolder));
                    result.push(UpdateAction::new(
                        parent_path(col_path),
                        UpdateActionKind::RefreshFolder,
                    ));
                } else {
                    result.push(UpdateAction::new(
                        self.inner.get_true_parent(col_path, path),
                        UpdateActionKind::RefreshFolder,
                    ))
                }
            }
            Modification::MovedTo(to_path) => {
                if self.inner.path_type(path).is_dir() {
                    if self.inner.is_collapsable_folder(&to_path) {
                        result.push(UpdateAction::new(col_path, UpdateActionKind::RemoveFolder));
                    } else {
                        let dest_path = self.inner.strip_base(&to_path).into();
                        result.push(UpdateAction::new(
                            col_path,
                            UpdateActionKind::RenameFolder { to: dest_path },
                        ));
                    }
                    let orig_parent = parent_path(col_path);
                    let new_parent = parent_path(self.inner.strip_base(&to_path));
                    let parents_differs = new_parent != orig_parent;
                    result.push(UpdateAction::new(
                        orig_parent,
                        UpdateActionKind::RefreshFolder,
                    ));
                    if parents_differs {
                        result.push(UpdateAction::new(
                            new_parent,
                            UpdateActionKind::RefreshFolder,
                        ));
                    }
                } else {
                    result.push(UpdateAction::new(
                        self.inner.get_true_parent(col_path, path),
                        UpdateActionKind::RefreshFolder,
                    ));
                    let dest_path = self.inner.strip_base(&to_path);
                    if self.inner.path_type(&to_path).is_dir() {
                        result.push(UpdateAction::new(
                            parent_path(dest_path),
                            UpdateActionKind::RefreshFolder,
                        ));
                        result.push(UpdateAction::new(
                            dest_path,
                            UpdateActionKind::RefreshFolderRecursive,
                        ))
                    } else {
                        result.push(UpdateAction::new(
                            self.inner.get_true_parent(dest_path, &to_path),
                            UpdateActionKind::RefreshFolder,
                        ))
                    }
                }
            }
        };

        result
    }

    fn insert_event(&mut self, event: Event) {
        if event.paths.is_empty() {
            error!("Event {:?} without path", event);
            return;
        }

        let mut paths_iter = event.paths.into_iter();
        let path = paths_iter.next().unwrap(); // safe as we checked emptiness of Vec before
                                               // debounce delete folder - can delete all events in it
        if let EventKind::Remove(RemoveKind::Folder) = event.kind {
            self.pending.retain(|p, _| p.starts_with(&path));
        }

        let detected_modification = Modification::from_event_kind(event.kind, paths_iter.next());
        let detected_modification = match detected_modification {
            Some(m) => m,
            None => {
                warn!(
                    "Event without modification detected: {:?} on {:?}",
                    event.kind, path
                );
                return;
            }
        };
        if let Modification::MovedTo(ref path) = detected_modification {
            //delete change in moved to, as contained in this change
            self.pending.remove(path);
        }

        let now = Instant::now();
        let entry = self.pending.entry(path);
        let mut to_be_deleted: Option<PathBuf> = None;
        match entry {
            std::collections::hash_map::Entry::Occupied(_) => {
                let mut delete_this = false;
                let entry = entry.and_modify(|evt| {
                    evt.last_change = now;
                    match (&evt.change_type, &detected_modification) {
                        // do not overwrite creation with modification
                        (Modification::Created, Modification::Modified) => (),
                        // if was created and then deleted can remove it from events
                        (Modification::Created, Modification::Deleted) => {
                            delete_this = true;
                        }
                        _ => evt.change_type = detected_modification,
                    };
                });

                if delete_this {
                    to_be_deleted = Some(entry.key().clone());
                }
            }
            std::collections::hash_map::Entry::Vacant(_) => {
                entry.or_insert(PendingEvent {
                    last_change: now,
                    change_type: detected_modification,
                });
            }
        };

        if let Some(path_to_delete) = to_be_deleted {
            self.pending.remove(&path_to_delete);
        }
    }

    pub(super) fn run_event_loop(mut self) {
        loop {
            match self.input_channel.recv_timeout(self.interval) {
                Ok(Some(action)) => {
                    self.insert_event(action);

                    if self.pending.len() >= 10_000 {
                        // should not grow too big
                        self.send_update_actions(false);
                    }
                }
                Ok(None) => {
                    self.send_update_actions(true);
                    return;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    error!("OngoingUpdater channel disconnected preliminary");
                    self.send_update_actions(true);
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {
                    //every pending action is older then limit
                    self.send_update_actions(false);
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
    Pass(Event),
    Error(notify::Error, Option<PathBuf>),
    Ignore,
    Rescan,
}

pub(crate) fn filter_event(evt: Result<Event, notify::Error>) -> FilteredEvent {
    use FilteredEvent::*;
    match evt {
        Ok(evt) => {
            if evt.need_rescan() {
                Rescan
            } else {
                match evt.kind {
                    EventKind::Any | EventKind::Access(_) | EventKind::Other => Ignore,
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => Pass(evt),
                }
            }
        }
        Err(e) => Error(e, None),
    }
}
