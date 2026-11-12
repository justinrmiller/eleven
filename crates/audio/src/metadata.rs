//! Tag and duration scanning for playlist rows, on background threads.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use eleven_core::EntryId;
use lofty::prelude::{Accessor, AudioFile, TaggedFileExt};

use crate::decoder::Decoder;
use crate::engine::Notify;

/// What the playlist needs to know about a file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagInfo {
    /// Track title.
    pub title: Option<String>,
    /// Track artist.
    pub artist: Option<String>,
    /// Album name.
    pub album: Option<String>,
    /// Playing time.
    pub duration: Option<Duration>,
}

fn non_empty(s: Option<std::borrow::Cow<'_, str>>) -> Option<String> {
    s.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

/// Reads tags with lofty, falling back to probing the file with symphonia for the duration.
/// Returns `None` if the file is unreadable by both.
#[must_use]
pub fn read_tags(path: &Path) -> Option<TagInfo> {
    match lofty::read_from_path(path) {
        Ok(tagged) => {
            let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
            let duration = Some(tagged.properties().duration()).filter(|d| !d.is_zero());
            let mut info = TagInfo {
                title: tag.and_then(|t| non_empty(t.title())),
                artist: tag.and_then(|t| non_empty(t.artist())),
                album: tag.and_then(|t| non_empty(t.album())),
                duration,
            };
            if info.duration.is_none() {
                info.duration = Decoder::open_file(path)
                    .ok()
                    .and_then(|d| d.info().duration);
            }
            Some(info)
        }
        Err(_) => Decoder::open_file(path).ok().map(|d| TagInfo {
            duration: d.info().duration,
            ..TagInfo::default()
        }),
    }
}

/// A pool of threads that read tags for playlist entries.
pub struct MetadataScanner {
    requests: Sender<(EntryId, PathBuf)>,
    results: Receiver<(EntryId, TagInfo)>,
}

impl std::fmt::Debug for MetadataScanner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetadataScanner")
            .field("queued", &self.requests.len())
            .finish()
    }
}

impl MetadataScanner {
    /// Starts `workers` scanning threads. `notify` is called after each result.
    #[must_use]
    pub fn new(workers: usize, notify: &Notify) -> Self {
        let (req_tx, req_rx) = crossbeam_channel::unbounded::<(EntryId, PathBuf)>();
        let (res_tx, res_rx) = crossbeam_channel::unbounded();
        for i in 0..workers.max(1) {
            let rx = req_rx.clone();
            let tx = res_tx.clone();
            let notify = std::sync::Arc::clone(notify);
            let _ = std::thread::Builder::new()
                .name(format!("eleven-tags-{i}"))
                .spawn(move || {
                    for (id, path) in rx {
                        let info = read_tags(&path).unwrap_or_default();
                        if tx.send((id, info)).is_err() {
                            break;
                        }
                        notify();
                    }
                });
        }
        Self {
            requests: req_tx,
            results: res_rx,
        }
    }

    /// Queues a file for scanning.
    pub fn request(&self, id: EntryId, path: PathBuf) {
        let _ = self.requests.send((id, path));
    }

    /// Takes all finished results.
    #[must_use]
    pub fn poll(&self) -> Vec<(EntryId, TagInfo)> {
        self.results.try_iter().collect()
    }

    /// Waits up to `timeout` for one result (for tests).
    #[must_use]
    pub fn wait(&self, timeout: Duration) -> Option<(EntryId, TagInfo)> {
        self.results.recv_timeout(timeout).ok()
    }
}
