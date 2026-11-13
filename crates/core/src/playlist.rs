//! The playlist model: entries, selection, the current track and shuffle/repeat navigation.
//!
//! Entries are identified by a stable [`EntryId`] so that the current track, the shuffle order
//! and asynchronous metadata updates survive sorting, moving and removal of other entries.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where a playlist entry's audio comes from.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Location {
    /// A file on the local file system.
    File(PathBuf),
    /// A network stream (`http://` or `https://`).
    Url(String),
}

impl Location {
    /// Interprets a playlist line or user input as a location.
    ///
    /// `http(s)://` strings become [`Location::Url`]; `file://` URLs are decoded to paths; other
    /// strings are treated as paths, resolved against `base_dir` when relative.
    #[must_use]
    pub fn parse(input: &str, base_dir: Option<&Path>) -> Self {
        let trimmed = input.trim();
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            return Self::Url(trimmed.to_owned());
        }
        if lower.starts_with("file://") {
            return Self::File(PathBuf::from(percent_decode(&trimmed["file://".len()..])));
        }
        let path = PathBuf::from(trimmed);
        match base_dir {
            Some(base) if path.is_relative() => Self::File(base.join(path)),
            _ => Self::File(path),
        }
    }

    /// Returns `true` for network streams.
    #[must_use]
    pub fn is_url(&self) -> bool {
        matches!(self, Self::Url(_))
    }

    /// A short human-readable name: the file stem for files, the URL for streams.
    #[must_use]
    pub fn display_name(&self) -> String {
        match self {
            Self::File(path) => path.file_stem().or_else(|| path.file_name()).map_or_else(
                || path.display().to_string(),
                |s| s.to_string_lossy().into_owned(),
            ),
            Self::Url(url) => url.clone(),
        }
    }

    /// The full path or URL as a string, as written to playlist files.
    #[must_use]
    pub fn as_playlist_string(&self) -> String {
        match self {
            Self::File(path) => path.display().to_string(),
            Self::Url(url) => url.clone(),
        }
    }

    /// The lower-cased file extension, if this is a file with one.
    #[must_use]
    pub fn extension(&self) -> Option<String> {
        match self {
            Self::File(path) => path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase()),
            Self::Url(_) => None,
        }
    }
}

/// Decodes `%XX` escapes (as used in `file://` URLs). Invalid escapes are kept verbatim.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Stable identifier of a playlist entry, unique within one [`Playlist`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryId(u64);

/// One row of the playlist.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    id: EntryId,
    /// Where the audio lives.
    pub location: Location,
    /// Title from tags or the playlist file.
    pub title: Option<String>,
    /// Artist from tags.
    pub artist: Option<String>,
    /// Track length, once known.
    pub duration: Option<Duration>,
    /// Whether the row is selected in the playlist editor.
    pub selected: bool,
}

impl Entry {
    /// The entry's stable identifier.
    #[must_use]
    pub fn id(&self) -> EntryId {
        self.id
    }

    /// The text shown in the playlist: `"Artist - Title"`, the title, or the file name.
    #[must_use]
    pub fn display_title(&self) -> String {
        match (&self.artist, &self.title) {
            (Some(artist), Some(title)) if !artist.is_empty() && !title.is_empty() => {
                format!("{artist} - {title}")
            }
            (_, Some(title)) if !title.is_empty() => title.clone(),
            _ => self.location.display_name(),
        }
    }
}

/// How a click modifies the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectMode {
    /// Select only this row (plain click).
    Single,
    /// Toggle this row, keep the rest (Ctrl/Cmd-click).
    Toggle,
    /// Select the range from the anchor to this row (Shift-click).
    Range,
}

/// Keys the playlist can be sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    /// The displayed title, case-insensitive.
    Title,
    /// The file name (or URL), case-insensitive.
    Filename,
    /// The full path (or URL), case-insensitive.
    Path,
    /// Track length, unknown lengths last.
    Duration,
}

/// Sum of a set of durations, noting whether any were unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TotalDuration {
    /// Sum of all known durations.
    pub known: Duration,
    /// `true` if at least one entry has no known duration.
    pub has_unknown: bool,
}

/// An ordered list of tracks with selection, a current track, and shuffle/repeat navigation.
#[derive(Debug, Clone)]
pub struct Playlist {
    entries: Vec<Entry>,
    current: Option<EntryId>,
    anchor: Option<usize>,
    shuffle: bool,
    repeat: bool,
    /// A permutation of all entry ids; the play order used while shuffle is on.
    shuffle_order: Vec<EntryId>,
    rng: fastrand::Rng,
    next_id: u64,
}

impl Default for Playlist {
    fn default() -> Self {
        Self::new()
    }
}

impl Playlist {
    /// Creates an empty playlist with a randomly seeded shuffle generator.
    #[must_use]
    pub fn new() -> Self {
        Self::with_rng(fastrand::Rng::new())
    }

    /// Creates an empty playlist with a deterministic shuffle generator (for tests).
    #[must_use]
    pub fn with_seed(seed: u64) -> Self {
        Self::with_rng(fastrand::Rng::with_seed(seed))
    }

    fn with_rng(rng: fastrand::Rng) -> Self {
        Self {
            entries: Vec::new(),
            current: None,
            anchor: None,
            shuffle: false,
            repeat: false,
            shuffle_order: Vec::new(),
            rng,
            next_id: 0,
        }
    }

    // ----------------------------------------------------------------------------------------
    // Accessors

    /// All entries in display order.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when the playlist has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entry at `index`, if any.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Entry> {
        self.entries.get(index)
    }

    /// Index of the entry with the given id.
    #[must_use]
    pub fn index_of(&self, id: EntryId) -> Option<usize> {
        self.entries.iter().position(|e| e.id == id)
    }

    /// Index of the current track.
    #[must_use]
    pub fn current_index(&self) -> Option<usize> {
        self.current.and_then(|id| self.index_of(id))
    }

    /// The current track's id.
    #[must_use]
    pub fn current_id(&self) -> Option<EntryId> {
        self.current
    }

    /// The current track.
    #[must_use]
    pub fn current(&self) -> Option<&Entry> {
        self.current_index().map(|i| &self.entries[i])
    }

    /// Whether shuffle is enabled.
    #[must_use]
    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    /// Whether repeat is enabled.
    #[must_use]
    pub fn repeat(&self) -> bool {
        self.repeat
    }

    /// The current shuffle play order (a permutation of all entry ids).
    #[must_use]
    pub fn shuffle_order(&self) -> &[EntryId] {
        &self.shuffle_order
    }

    // ----------------------------------------------------------------------------------------
    // Adding and removing

    /// Appends a new entry and returns its id.
    pub fn push(&mut self, location: Location) -> EntryId {
        self.insert(self.entries.len(), location, None, None)
    }

    /// Appends a new entry with a known title and duration (e.g. from an `#EXTINF` line).
    pub fn push_with(
        &mut self,
        location: Location,
        title: Option<String>,
        duration: Option<Duration>,
    ) -> EntryId {
        self.insert(self.entries.len(), location, title, duration)
    }

    /// Inserts a new entry at `index` (clamped to the end) and returns its id.
    pub fn insert(
        &mut self,
        index: usize,
        location: Location,
        title: Option<String>,
        duration: Option<Duration>,
    ) -> EntryId {
        let id = EntryId(self.next_id);
        self.next_id += 1;
        let index = index.min(self.entries.len());
        self.entries.insert(
            index,
            Entry {
                id,
                location,
                title,
                artist: None,
                duration,
                selected: false,
            },
        );
        // Insert the new id at a random point after the current track in the shuffle order,
        // so newly added songs are still "to come" without restarting the shuffle.
        let start = self.current_shuffle_pos().map_or(0, |p| p + 1);
        let pos = self.rng.usize(start..=self.shuffle_order.len());
        self.shuffle_order.insert(pos, id);
        self.anchor = None;
        id
    }

    /// Removes the entries at the given indices (out-of-range indices are ignored).
    ///
    /// If the current track is removed the playlist has no current track afterwards; the next
    /// call to [`Playlist::next_track`] starts from the beginning of the play order.
    pub fn remove_indices(&mut self, indices: &[usize]) {
        let mut doomed = vec![false; self.entries.len()];
        for &i in indices {
            if let Some(flag) = doomed.get_mut(i) {
                *flag = true;
            }
        }
        let mut i = 0;
        self.entries.retain(|_| {
            let keep = !doomed[i];
            i += 1;
            keep
        });
        self.after_removal();
    }

    /// Removes all selected entries.
    pub fn remove_selected(&mut self) {
        self.entries.retain(|e| !e.selected);
        self.after_removal();
    }

    /// Removes all entries that are *not* selected.
    pub fn crop(&mut self) {
        self.entries.retain(|e| e.selected);
        self.after_removal();
    }

    /// Removes every entry.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.after_removal();
    }

    fn after_removal(&mut self) {
        let entries = &self.entries;
        self.shuffle_order
            .retain(|id| entries.iter().any(|e| e.id == *id));
        if let Some(id) = self.current
            && self.index_of(id).is_none()
        {
            self.current = None;
        }
        self.anchor = None;
    }

    // ----------------------------------------------------------------------------------------
    // Metadata

    /// Updates tag information for an entry; returns `false` if the entry no longer exists.
    pub fn update_metadata(
        &mut self,
        id: EntryId,
        title: Option<String>,
        artist: Option<String>,
        duration: Option<Duration>,
    ) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) else {
            return false;
        };
        if title.is_some() {
            entry.title = title;
        }
        if artist.is_some() {
            entry.artist = artist;
        }
        if duration.is_some() {
            entry.duration = duration;
        }
        true
    }

    /// Replaces an entry's tag information (title and artist, even when absent); a `None`
    /// duration keeps the current one. Use this for authoritative tag reads, so a title saved
    /// in a playlist file ("Artist - Title") is not combined with the tags again. Returns
    /// `false` if the entry no longer exists.
    pub fn set_tags(
        &mut self,
        id: EntryId,
        title: Option<String>,
        artist: Option<String>,
        duration: Option<Duration>,
    ) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) else {
            return false;
        };
        entry.title = title;
        entry.artist = artist;
        if duration.is_some() {
            entry.duration = duration;
        }
        true
    }

    /// Sum of all entry durations.
    #[must_use]
    pub fn total_duration(&self) -> TotalDuration {
        sum_durations(self.entries.iter())
    }

    /// Sum of the selected entries' durations.
    #[must_use]
    pub fn selected_duration(&self) -> TotalDuration {
        sum_durations(self.entries.iter().filter(|e| e.selected))
    }

    // ----------------------------------------------------------------------------------------
    // Selection

    /// Changes the selection as a click on row `index` would.
    pub fn select(&mut self, index: usize, mode: SelectMode) {
        if index >= self.entries.len() {
            return;
        }
        match mode {
            SelectMode::Single => {
                for (i, e) in self.entries.iter_mut().enumerate() {
                    e.selected = i == index;
                }
                self.anchor = Some(index);
            }
            SelectMode::Toggle => {
                self.entries[index].selected = !self.entries[index].selected;
                self.anchor = Some(index);
            }
            SelectMode::Range => {
                let anchor = self
                    .anchor
                    .filter(|&a| a < self.entries.len())
                    .unwrap_or(index);
                let (lo, hi) = if anchor <= index {
                    (anchor, index)
                } else {
                    (index, anchor)
                };
                for (i, e) in self.entries.iter_mut().enumerate() {
                    e.selected = (lo..=hi).contains(&i);
                }
                self.anchor = Some(anchor);
            }
        }
    }

    /// Selects every entry.
    pub fn select_all(&mut self) {
        self.entries.iter_mut().for_each(|e| e.selected = true);
    }

    /// Deselects every entry.
    pub fn select_none(&mut self) {
        self.entries.iter_mut().for_each(|e| e.selected = false);
        self.anchor = None;
    }

    /// Inverts the selection.
    pub fn invert_selection(&mut self) {
        self.entries
            .iter_mut()
            .for_each(|e| e.selected = !e.selected);
    }

    /// Indices of all selected entries, ascending.
    #[must_use]
    pub fn selected_indices(&self) -> Vec<usize> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.selected)
            .map(|(i, _)| i)
            .collect()
    }

    // ----------------------------------------------------------------------------------------
    // Reordering

    /// Moves every selected entry one row up (`delta < 0`) or down (`delta > 0`).
    ///
    /// A selected block that already touches the edge stays put, as in XMMS.
    pub fn move_selected(&mut self, delta: isize) {
        let n = self.entries.len();
        if n < 2 || delta == 0 {
            return;
        }
        for _ in 0..delta.unsigned_abs() {
            if delta < 0 {
                for i in 1..n {
                    if self.entries[i].selected && !self.entries[i - 1].selected {
                        self.entries.swap(i, i - 1);
                    }
                }
            } else {
                for i in (0..n - 1).rev() {
                    if self.entries[i].selected && !self.entries[i + 1].selected {
                        self.entries.swap(i, i + 1);
                    }
                }
            }
        }
        self.anchor = None;
    }

    /// Sorts the playlist (stable) by `key`.
    pub fn sort_by(&mut self, key: SortKey) {
        match key {
            SortKey::Title => self
                .entries
                .sort_by_cached_key(|e| e.display_title().to_lowercase()),
            SortKey::Filename => self.entries.sort_by_cached_key(|e| match &e.location {
                Location::File(p) => p
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().to_lowercase()),
                Location::Url(u) => u.to_lowercase(),
            }),
            SortKey::Path => {
                self.entries
                    .sort_by_cached_key(|e| e.location.as_playlist_string().to_lowercase());
            }
            SortKey::Duration => self
                .entries
                .sort_by_key(|e| (e.duration.is_none(), e.duration)),
        }
        self.anchor = None;
    }

    /// Reverses the order of the entries.
    pub fn reverse(&mut self) {
        self.entries.reverse();
        self.anchor = None;
    }

    /// Physically shuffles the entries (the "Randomize list" command).
    pub fn randomize(&mut self) {
        self.rng.shuffle(&mut self.entries);
        self.anchor = None;
    }

    // ----------------------------------------------------------------------------------------
    // Navigation

    /// Enables or disables shuffle. Enabling it draws a fresh play order starting at the
    /// current track.
    pub fn set_shuffle(&mut self, on: bool) {
        if on && !self.shuffle {
            self.reshuffle();
        }
        self.shuffle = on;
    }

    /// Enables or disables repeat (wrap around at the end of the list).
    pub fn set_repeat(&mut self, on: bool) {
        self.repeat = on;
    }

    /// Makes the entry at `index` current (e.g. on double-click). Returns its id.
    ///
    /// The chosen track also moves to the current point of the shuffle order (right after the
    /// previous current track), so jumping around never strands unplayed tracks behind it.
    pub fn set_current(&mut self, index: usize) -> Option<EntryId> {
        let id = self.entries.get(index)?.id;
        if self.current != Some(id)
            && let Some(from) = self.shuffle_order.iter().position(|&s| s == id)
        {
            self.shuffle_order.remove(from);
            let to = self.current_shuffle_pos().map_or(0, |p| p + 1);
            self.shuffle_order.insert(to, id);
        }
        self.current = Some(id);
        Some(id)
    }

    /// Clears the current track.
    pub fn clear_current(&mut self) {
        self.current = None;
    }

    /// Advances to the next track according to shuffle/repeat and makes it current.
    ///
    /// Returns the new current index, or `None` at the end of the list (repeat off) or when the
    /// playlist is empty. At the end of the list the current track is left unchanged.
    pub fn next_track(&mut self) -> Option<usize> {
        if self.entries.is_empty() {
            return None;
        }
        let next_id = if self.shuffle {
            match self.current_shuffle_pos() {
                None => self.shuffle_order[0],
                Some(p) if p + 1 < self.shuffle_order.len() => self.shuffle_order[p + 1],
                Some(_) if self.repeat => {
                    self.reshuffle_avoiding_current();
                    self.shuffle_order[0]
                }
                Some(_) => return None,
            }
        } else {
            match self.current_index() {
                None => self.entries[0].id,
                Some(i) if i + 1 < self.entries.len() => self.entries[i + 1].id,
                Some(_) if self.repeat => self.entries[0].id,
                Some(_) => return None,
            }
        };
        self.current = Some(next_id);
        self.current_index()
    }

    /// Goes back to the previous track according to shuffle/repeat and makes it current.
    ///
    /// Returns the new current index, or `None` at the start of the list (repeat off).
    pub fn prev_track(&mut self) -> Option<usize> {
        if self.entries.is_empty() {
            return None;
        }
        let prev_id = if self.shuffle {
            let last = self.shuffle_order.len() - 1;
            match self.current_shuffle_pos() {
                None => self.shuffle_order[0],
                Some(0) if self.repeat => self.shuffle_order[last],
                Some(0) => return None,
                Some(p) => self.shuffle_order[p - 1],
            }
        } else {
            let last = self.entries.len() - 1;
            match self.current_index() {
                None => self.entries[0].id,
                Some(0) if self.repeat => self.entries[last].id,
                Some(0) => return None,
                Some(i) => self.entries[i - 1].id,
            }
        };
        self.current = Some(prev_id);
        self.current_index()
    }

    fn current_shuffle_pos(&self) -> Option<usize> {
        let id = self.current?;
        self.shuffle_order.iter().position(|&s| s == id)
    }

    /// Draws a new random play order with the current track (if any) first.
    fn reshuffle(&mut self) {
        let mut order: Vec<EntryId> = self.entries.iter().map(|e| e.id).collect();
        self.rng.shuffle(&mut order);
        if let Some(cur) = self.current
            && let Some(pos) = order.iter().position(|&id| id == cur)
        {
            order.swap(0, pos);
        }
        self.shuffle_order = order;
    }

    /// Draws a new play order for the next lap, avoiding replaying the current track first.
    fn reshuffle_avoiding_current(&mut self) {
        let mut order: Vec<EntryId> = self.entries.iter().map(|e| e.id).collect();
        self.rng.shuffle(&mut order);
        if order.len() > 1
            && let Some(cur) = self.current
            && order[0] == cur
        {
            let swap_with = self.rng.usize(1..order.len());
            order.swap(0, swap_with);
        }
        self.shuffle_order = order;
    }

    // ----------------------------------------------------------------------------------------
    // Searching

    /// Indices of entries matching a jump-to-file query.
    ///
    /// The query is split on whitespace; every term must appear (case-insensitively) in the
    /// entry's display title or its path/URL. An empty query matches everything.
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<usize> {
        let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                let hay = format!(
                    "{}\n{}",
                    e.display_title().to_lowercase(),
                    e.location.as_playlist_string().to_lowercase()
                );
                terms.iter().all(|t| hay.contains(t.as_str()))
            })
            .map(|(i, _)| i)
            .collect()
    }
}

fn sum_durations<'a>(entries: impl Iterator<Item = &'a Entry>) -> TotalDuration {
    entries.fold(TotalDuration::default(), |mut acc, e| {
        match e.duration {
            Some(d) => acc.known += d,
            None => acc.has_unknown = true,
        }
        acc
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn file(name: &str) -> Location {
        Location::File(PathBuf::from(format!("/music/{name}.mp3")))
    }

    fn playlist(n: usize) -> Playlist {
        let mut p = Playlist::with_seed(11);
        for i in 0..n {
            p.push(file(&format!("track{i:02}")));
        }
        p
    }

    fn names(p: &Playlist) -> Vec<String> {
        p.entries()
            .iter()
            .map(|e| e.location.display_name())
            .collect()
    }

    #[test]
    fn location_parse_variants() {
        assert_eq!(
            Location::parse("http://radio.example/stream", None),
            Location::Url("http://radio.example/stream".into())
        );
        assert!(Location::parse("HTTPS://X", None).is_url());
        assert_eq!(
            Location::parse("file:///tmp/My%20Song.flac", None),
            Location::File("/tmp/My Song.flac".into())
        );
        assert_eq!(
            Location::parse("sub/a.ogg", Some(Path::new("/base"))),
            Location::File("/base/sub/a.ogg".into())
        );
        assert_eq!(
            Location::parse("/abs/a.ogg", Some(Path::new("/base"))),
            Location::File("/abs/a.ogg".into())
        );
        assert_eq!(
            Location::parse("  x.wav \r", None),
            Location::File("x.wav".into())
        );
    }

    #[test]
    fn percent_decode_handles_bad_escapes() {
        assert_eq!(percent_decode("a%2Fb"), "a/b");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
    }

    #[test]
    fn location_names_and_extensions() {
        let f = Location::File("/a/b/Song.Name.MP3".into());
        assert_eq!(f.display_name(), "Song.Name");
        assert_eq!(f.extension().as_deref(), Some("mp3"));
        let u = Location::Url("http://x/y".into());
        assert_eq!(u.display_name(), "http://x/y");
        assert_eq!(u.extension(), None);
        assert_eq!(u.as_playlist_string(), "http://x/y");
    }

    #[test]
    fn display_title_prefers_tags() {
        let mut p = playlist(1);
        let id = p.entries()[0].id();
        assert_eq!(p.entries()[0].display_title(), "track00");
        p.update_metadata(id, Some("Tonight".into()), None, None);
        assert_eq!(p.entries()[0].display_title(), "Tonight");
        p.update_metadata(
            id,
            None,
            Some("Spinal Tap".into()),
            Some(Duration::from_secs(3)),
        );
        assert_eq!(p.entries()[0].display_title(), "Spinal Tap - Tonight");
        assert_eq!(p.entries()[0].duration, Some(Duration::from_secs(3)));
        p.clear();
        assert!(!p.update_metadata(id, None, None, None));
    }

    #[test]
    fn linear_navigation_and_repeat() {
        let mut p = playlist(3);
        assert_eq!(p.current_index(), None);
        assert_eq!(p.next_track(), Some(0));
        assert_eq!(p.next_track(), Some(1));
        assert_eq!(p.next_track(), Some(2));
        assert_eq!(p.next_track(), None, "end of list without repeat");
        assert_eq!(
            p.current_index(),
            Some(2),
            "current stays on the last track"
        );
        p.set_repeat(true);
        assert!(p.repeat());
        assert_eq!(p.next_track(), Some(0));
        assert_eq!(p.prev_track(), Some(2));
        p.set_repeat(false);
        assert_eq!(p.prev_track(), Some(1));
        assert_eq!(p.prev_track(), Some(0));
        assert_eq!(p.prev_track(), None);
    }

    #[test]
    fn navigation_on_empty_playlist() {
        let mut p = Playlist::new();
        assert!(p.is_empty());
        assert_eq!(p.next_track(), None);
        assert_eq!(p.prev_track(), None);
        assert_eq!(p.set_current(0), None);
        p.set_shuffle(true);
        assert_eq!(p.next_track(), None);
    }

    #[test]
    fn prev_without_current_starts_at_beginning() {
        let mut p = playlist(3);
        assert_eq!(p.prev_track(), Some(0));
        let mut p = playlist(3);
        p.set_shuffle(true);
        let first = p.shuffle_order()[0];
        p.prev_track();
        assert_eq!(p.current_id(), Some(first));
    }

    #[test]
    fn shuffle_visits_every_track_once_per_lap() {
        let mut p = playlist(10);
        p.set_current(4);
        p.set_shuffle(true);
        assert!(p.shuffle());
        assert_eq!(
            p.shuffle_order()[0],
            p.entries()[4].id(),
            "current track starts the order"
        );
        let mut seen = vec![p.current_index().unwrap()];
        while let Some(i) = p.next_track() {
            seen.push(i);
        }
        seen.sort_unstable();
        assert_eq!(seen, (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn shuffle_repeat_starts_new_lap_with_a_different_track() {
        let mut p = playlist(5);
        p.set_shuffle(true);
        p.set_repeat(true);
        for _ in 0..5 {
            p.next_track();
        }
        for _ in 0..20 {
            // Walk to the end of the lap and one step beyond.
            while p.current_id() != p.shuffle_order().last().copied() {
                p.next_track();
            }
            let end = p.current_id();
            p.next_track();
            assert_ne!(
                p.current_id(),
                end,
                "a new lap must not replay the last track"
            );
        }
    }

    #[test]
    fn shuffle_prev_walks_back_and_wraps() {
        let mut p = playlist(4);
        p.set_shuffle(true);
        p.next_track();
        p.next_track();
        let second = p.current_id();
        p.next_track();
        p.prev_track();
        assert_eq!(p.current_id(), second);
        p.prev_track();
        assert_eq!(p.prev_track(), None);
        p.set_repeat(true);
        p.prev_track();
        assert_eq!(p.current_id(), p.shuffle_order().last().copied());
    }

    #[test]
    fn single_track_shuffle_repeat_replays() {
        let mut p = playlist(1);
        p.set_shuffle(true);
        p.set_repeat(true);
        assert_eq!(p.next_track(), Some(0));
        assert_eq!(p.next_track(), Some(0));
    }

    #[test]
    fn jumping_in_shuffle_mode_strands_no_tracks() {
        for jump_to in 0..10 {
            let mut p = playlist(10);
            p.set_shuffle(true);
            let mut played = vec![p.next_track().unwrap(), p.next_track().unwrap()];
            p.set_current(jump_to);
            played.push(jump_to);
            while let Some(i) = p.next_track() {
                played.push(i);
            }
            played.sort_unstable();
            played.dedup();
            assert_eq!(played, (0..10).collect::<Vec<_>>(), "jump to {jump_to}");
        }
    }

    #[test]
    fn restored_current_track_starts_the_shuffle_order() {
        // Controller::new enables shuffle on an empty list, adds entries, then restores.
        let mut p = Playlist::with_seed(5);
        p.set_shuffle(true);
        for i in 0..8 {
            p.push(file(&format!("t{i}")));
        }
        p.set_current(6);
        assert_eq!(p.shuffle_order()[0], p.entries()[6].id());
        let mut seen = vec![6];
        while let Some(i) = p.next_track() {
            seen.push(i);
        }
        assert_eq!(seen.len(), 8);
    }

    #[test]
    fn set_tags_replaces_title_and_artist() {
        let mut p = playlist(1);
        let id = p.entries()[0].id();
        p.update_metadata(
            id,
            Some("A - song".into()),
            None,
            Some(Duration::from_secs(9)),
        );
        assert!(p.set_tags(id, None, Some("A".into()), None));
        let e = &p.entries()[0];
        assert_eq!((e.title.as_deref(), e.artist.as_deref()), (None, Some("A")));
        assert_eq!(
            e.duration,
            Some(Duration::from_secs(9)),
            "None keeps the duration"
        );
        assert_eq!(e.display_title(), "track00");
        p.clear();
        assert!(!p.set_tags(id, None, None, None));
    }

    #[test]
    fn current_follows_sorting_and_moving() {
        let mut p = Playlist::with_seed(1);
        for n in ["c", "a", "b"] {
            p.push(file(n));
        }
        p.set_current(0); // "c"
        p.sort_by(SortKey::Filename);
        assert_eq!(names(&p), ["a", "b", "c"]);
        assert_eq!(p.current_index(), Some(2));
        p.reverse();
        assert_eq!(p.current_index(), Some(0));
        assert_eq!(p.current().unwrap().location.display_name(), "c");
    }

    #[test]
    fn removing_current_clears_it() {
        let mut p = playlist(3);
        p.set_current(1);
        p.remove_indices(&[1, 99]);
        assert_eq!(p.len(), 2);
        assert_eq!(p.current_index(), None);
        assert_eq!(p.next_track(), Some(0));
        p.clear_current();
        assert_eq!(p.current(), None);
    }

    #[test]
    fn remove_crop_and_clear() {
        let mut p = playlist(5);
        p.select(1, SelectMode::Single);
        p.select(3, SelectMode::Toggle);
        assert_eq!(p.selected_indices(), [1, 3]);
        p.remove_selected();
        assert_eq!(names(&p), ["track00", "track02", "track04"]);
        p.select(1, SelectMode::Single);
        p.crop();
        assert_eq!(names(&p), ["track02"]);
        assert_eq!(p.shuffle_order().len(), 1);
        p.clear();
        assert!(p.is_empty());
        assert!(p.shuffle_order().is_empty());
    }

    #[test]
    fn selection_modes() {
        let mut p = playlist(6);
        p.select(2, SelectMode::Single);
        p.select(4, SelectMode::Range);
        assert_eq!(p.selected_indices(), [2, 3, 4]);
        p.select(0, SelectMode::Range);
        assert_eq!(
            p.selected_indices(),
            [0, 1, 2],
            "range is anchored at the first click"
        );
        p.select(5, SelectMode::Toggle);
        assert_eq!(p.selected_indices(), [0, 1, 2, 5]);
        p.select(5, SelectMode::Toggle);
        assert_eq!(p.selected_indices(), [0, 1, 2]);
        p.invert_selection();
        assert_eq!(p.selected_indices(), [3, 4, 5]);
        p.select_all();
        assert_eq!(p.selected_indices().len(), 6);
        p.select_none();
        assert!(p.selected_indices().is_empty());
        p.select(99, SelectMode::Single);
        assert!(
            p.selected_indices().is_empty(),
            "out-of-range clicks are ignored"
        );
        // A range click with no anchor selects just that row.
        p.select(3, SelectMode::Range);
        assert_eq!(p.selected_indices(), [3]);
    }

    #[test]
    fn move_selected_respects_edges() {
        let mut p = playlist(5);
        p.select(0, SelectMode::Single);
        p.select(2, SelectMode::Toggle);
        p.move_selected(-1);
        assert_eq!(
            names(&p),
            ["track00", "track02", "track01", "track03", "track04"]
        );
        p.move_selected(-1);
        assert_eq!(
            names(&p),
            ["track00", "track02", "track01", "track03", "track04"]
        );
        p.move_selected(3);
        assert_eq!(
            names(&p),
            ["track01", "track03", "track04", "track00", "track02"]
        );
        p.move_selected(0);
        assert_eq!(p.selected_indices(), [3, 4]);
    }

    #[test]
    fn sorting_by_every_key() {
        let mut p = Playlist::with_seed(3);
        let a = p.push(Location::File("/z/b.mp3".into()));
        let b = p.push(Location::File("/a/c.mp3".into()));
        let c = p.push(Location::Url("http://A".into()));
        p.update_metadata(a, Some("zeta".into()), None, Some(Duration::from_secs(5)));
        p.update_metadata(b, Some("Alpha".into()), None, None);
        p.update_metadata(c, Some("mid".into()), None, Some(Duration::from_secs(1)));
        p.sort_by(SortKey::Title);
        assert_eq!(
            p.entries().iter().map(Entry::id).collect::<Vec<_>>(),
            [b, c, a]
        );
        p.sort_by(SortKey::Path);
        // "/a/c.mp3" < "/z/b.mp3" < "http://a" ('/' sorts before 'h').
        assert_eq!(
            p.entries().iter().map(Entry::id).collect::<Vec<_>>(),
            [b, a, c]
        );
        p.sort_by(SortKey::Filename);
        assert_eq!(
            p.entries().iter().map(Entry::id).collect::<Vec<_>>(),
            [a, b, c]
        );
        p.sort_by(SortKey::Duration);
        assert_eq!(
            p.entries().iter().map(Entry::id).collect::<Vec<_>>(),
            [c, a, b]
        );
    }

    #[test]
    fn randomize_keeps_all_entries() {
        let mut p = playlist(20);
        let before: Vec<EntryId> = p.entries().iter().map(Entry::id).collect();
        p.randomize();
        let mut after: Vec<EntryId> = p.entries().iter().map(Entry::id).collect();
        assert_ne!(
            before, after,
            "20 entries should not shuffle to the same order"
        );
        after.sort();
        assert_eq!(before, after);
    }

    #[test]
    fn durations_are_summed() {
        let mut p = playlist(3);
        let ids: Vec<EntryId> = p.entries().iter().map(Entry::id).collect();
        p.update_metadata(ids[0], None, None, Some(Duration::from_secs(60)));
        p.update_metadata(ids[1], None, None, Some(Duration::from_secs(30)));
        let total = p.total_duration();
        assert_eq!(total.known, Duration::from_secs(90));
        assert!(total.has_unknown);
        p.select(1, SelectMode::Single);
        assert_eq!(
            p.selected_duration(),
            TotalDuration {
                known: Duration::from_secs(30),
                has_unknown: false
            }
        );
    }

    #[test]
    fn search_matches_all_terms() {
        let mut p = Playlist::with_seed(0);
        let a = p.push(Location::File("/m/Spinal Tap/Stonehenge.flac".into()));
        p.push(Location::File("/m/Other/Song.flac".into()));
        p.update_metadata(
            a,
            Some("Stonehenge".into()),
            Some("Spinal Tap".into()),
            None,
        );
        assert_eq!(p.search("stone TAP"), [0]);
        assert_eq!(p.search("flac"), [0, 1]);
        assert_eq!(p.search(""), [0, 1]);
        assert!(p.search("eleven").is_empty());
    }

    #[test]
    fn insert_clamps_index() {
        let mut p = playlist(2);
        p.insert(99, file("end"), Some("End".into()), None);
        p.insert(0, file("start"), None, None);
        assert_eq!(names(&p), ["start", "track00", "track01", "end"]);
        assert_eq!(p.get(3).unwrap().title.as_deref(), Some("End"));
        let id = p.push_with(file("x"), None, Some(Duration::from_secs(2)));
        assert_eq!(p.index_of(id), Some(4));
    }

    #[test]
    fn added_tracks_come_after_current_in_shuffle_order() {
        let mut p = playlist(5);
        p.set_shuffle(true);
        p.next_track();
        p.next_track();
        let pos = p
            .shuffle_order()
            .iter()
            .position(|&id| Some(id) == p.current_id())
            .unwrap();
        for i in 0..10 {
            let id = p.push(file(&format!("new{i}")));
            let new_pos = p.shuffle_order().iter().position(|&s| s == id).unwrap();
            assert!(new_pos > pos);
        }
    }

    #[derive(Debug, Clone)]
    enum Op {
        Push,
        Remove(usize),
        Select(usize, u8),
        RemoveSelected,
        Crop,
        Move(i8),
        Sort(u8),
        Reverse,
        Randomize,
        Next,
        Prev,
        SetCurrent(usize),
        Shuffle(bool),
        Repeat(bool),
    }

    fn op_strategy() -> impl Strategy<Value = Op> {
        prop_oneof![
            4 => Just(Op::Push),
            1 => (0usize..20).prop_map(Op::Remove),
            2 => (0usize..20, 0u8..3).prop_map(|(i, m)| Op::Select(i, m)),
            1 => Just(Op::RemoveSelected),
            1 => Just(Op::Crop),
            1 => (-3i8..=3).prop_map(Op::Move),
            1 => (0u8..4).prop_map(Op::Sort),
            1 => Just(Op::Reverse),
            1 => Just(Op::Randomize),
            3 => Just(Op::Next),
            2 => Just(Op::Prev),
            1 => (0usize..20).prop_map(Op::SetCurrent),
            1 => any::<bool>().prop_map(Op::Shuffle),
            1 => any::<bool>().prop_map(Op::Repeat),
        ]
    }

    proptest! {
        #[test]
        fn invariants_hold_under_random_operations(
            seed in any::<u64>(),
            ops in proptest::collection::vec(op_strategy(), 0..80),
        ) {
            let mut p = Playlist::with_seed(seed);
            for op in ops {
                match op {
                    Op::Push => { p.push(file("x")); }
                    Op::Remove(i) => p.remove_indices(&[i]),
                    Op::Select(i, m) => p.select(i, [SelectMode::Single, SelectMode::Toggle, SelectMode::Range][m as usize]),
                    Op::RemoveSelected => p.remove_selected(),
                    Op::Crop => p.crop(),
                    Op::Move(d) => p.move_selected(isize::from(d)),
                    Op::Sort(k) => p.sort_by([SortKey::Title, SortKey::Filename, SortKey::Path, SortKey::Duration][k as usize]),
                    Op::Reverse => p.reverse(),
                    Op::Randomize => p.randomize(),
                    Op::Next => {
                        if let Some(i) = p.next_track() { prop_assert!(i < p.len()); }
                    }
                    Op::Prev => {
                        if let Some(i) = p.prev_track() { prop_assert!(i < p.len()); }
                    }
                    Op::SetCurrent(i) => { p.set_current(i); }
                    Op::Shuffle(b) => p.set_shuffle(b),
                    Op::Repeat(b) => p.set_repeat(b),
                }
                // The current track, if any, exists.
                if let Some(id) = p.current_id() {
                    prop_assert!(p.index_of(id).is_some());
                }
                // The shuffle order is always a permutation of the entry ids.
                let mut order: Vec<EntryId> = p.shuffle_order().to_vec();
                let mut ids: Vec<EntryId> = p.entries().iter().map(Entry::id).collect();
                order.sort();
                ids.sort();
                prop_assert_eq!(order, ids.clone());
                // Ids are unique.
                ids.dedup();
                prop_assert_eq!(ids.len(), p.len());
                // With entries present and repeat on, next() always yields a track.
                if p.repeat() && !p.is_empty() {
                    let mut probe = p.clone();
                    prop_assert!(probe.next_track().is_some());
                }
            }
        }
    }
}
