//! Inventory / ender-chest snapshots (§2.11).
//!
//! `Inventory-Show` and `EnderChest-Show` do **not** render an item list into
//! the hover. They render a one-line text hint and a `RUN_COMMAND
//! "/trchat view <id>"` click; the real contents are shown by the server
//! opening a read-only container once the player clicks.
//!
//! Lifecycle (spec §2.11):
//!
//! * id = 12 hexadecimal characters,
//! * TTL = 5 minutes, cleaned lazily on create and on open,
//! * at most 100 entries; overflow evicts the **oldest inserted** one,
//! * an unknown or expired id reports `Function-Snapshot-Expired`,
//! * opening a live snapshot does **not** consume it.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// §2.11 — `SNAPSHOT_TTL`, five minutes.
pub const SNAPSHOT_TTL: Duration = Duration::from_secs(5 * 60);
/// §2.11 — at most this many live snapshots.
pub const MAX_SNAPSHOTS: usize = 100;

/// What a snapshot holds. Only what the read-only viewer needs.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub id: String,
    /// `Function-Inventory-Title` / `Function-EnderChest-Title` already
    /// formatted with the owner's name.
    pub title: String,
    /// `54` for the player inventory (9×6) or `27` for the ender chest (9×3).
    pub size: usize,
    /// One entry per slot: `Some(registry_key, count)` or `None` when empty.
    pub items: Vec<Option<(String, u8)>>,
    created: Instant,
}

impl Snapshot {
    fn is_expired(&self, now: Instant) -> bool {
        now.duration_since(self.created) >= SNAPSHOT_TTL
    }
}

/// Insertion-ordered store; the front is the oldest entry.
fn store() -> &'static Mutex<VecDeque<Snapshot>> {
    static STORE: std::sync::OnceLock<Mutex<VecDeque<Snapshot>>> = std::sync::OnceLock::new();
    STORE.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn lock() -> std::sync::MutexGuard<'static, VecDeque<Snapshot>> {
    store().lock().unwrap_or_else(|e| e.into_inner())
}

/// §2.11 — `expireSnapshots`: lazily drops every entry past its TTL.
pub fn expire_snapshots() {
    let now = Instant::now();
    let mut s = lock();
    s.retain(|snap| !snap.is_expired(now));
}

/// Registers a snapshot and returns its id. Enforces the TTL sweep and the
/// 100-entry cap (evicting the oldest insert) before inserting.
pub fn create(title: String, size: usize, items: Vec<Option<(String, u8)>>) -> String {
    expire_snapshots();
    let id = super::functions::create_snapshot_id();
    let mut s = lock();
    // §2.11 — overflow removes the **oldest inserted** entry.
    while s.len() >= MAX_SNAPSHOTS {
        s.pop_front();
    }
    s.push_back(Snapshot {
        id: id.clone(),
        title,
        size,
        items,
        created: Instant::now(),
    });
    id
}

/// §2.11 — `openSnapshot`. `None` means the caller must send
/// `Function-Snapshot-Expired`; `Some` carries the title and slot contents the
/// read-only viewer should show.
///
/// The entry is **not** consumed: upstream keeps it in a `LinkedHashMap` until
/// the TTL sweep or the 100-entry cap evicts it, so the same snapshot can be
/// reopened while it is live.
pub fn open(id: &str) -> Option<(String, usize, Vec<Option<(String, u8)>>)> {
    expire_snapshots();
    let s = lock();
    let snap = s.iter().find(|snap| snap.id == id)?;
    Some((snap.title.clone(), snap.size, snap.items.clone()))
}

/// §2.10 `createItemSnapshot` for `Item-Show` with `UI: true`.
///
/// The 3×9 viewer shows the *item's* container: 13 leading empty slots put the
/// item itself at index 13 (the centre of a 3×9), followed by whatever the
/// item's `CONTAINER` component holds.
///
/// Deliberately **not** subject to [`MAX_SNAPSHOTS`]: the spec notes that
/// `createItemSnapshot` skips the cap that `createSnapshot` applies.
pub fn create_item_snapshot(
    title: String,
    container: Vec<Option<(String, u8)>>,
) -> String {
    expire_snapshots();
    let id = super::functions::create_snapshot_id();
    let mut items: Vec<Option<(String, u8)>> = vec![None; ITEM_SNAPSHOT_CENTER];
    items.push(Some(container_first(container)));
    // Pad to the full 3×9 grid; `truncate` alone would leave a short vector.
    items.resize(ITEM_SNAPSHOT_SIZE, None);
    let mut s = lock();
    s.push_back(Snapshot {
        id: id.clone(),
        title,
        size: ITEM_SNAPSHOT_SIZE,
        items,
        created: Instant::now(),
    });
    id
}

/// §2.10 — the item snapshot is a 3×9 container with the item in the middle.
const ITEM_SNAPSHOT_SIZE: usize = 27;
/// Index 13 is the centre of a 3×9 grid.
const ITEM_SNAPSHOT_CENTER: usize = 13;

/// Returns the first present entry of `container`, or a placeholder when the
/// item carries no container contents. The viewer only needs the centre slot
/// to be occupied for the common case.
fn container_first(container: Vec<Option<(String, u8)>>) -> (String, u8) {
    container
        .into_iter()
        .flatten()
        .next()
        .unwrap_or_else(|| (String::new(), 0))
}

/// Test-only helper: how many snapshots are live right now.
#[cfg(test)]
pub fn len() -> usize {
    lock().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The store is process-global and tests run in parallel, so every test
    /// takes this lock for its whole body.
    fn reset() -> std::sync::MutexGuard<'static, ()> {
        static SERIAL: Mutex<()> = Mutex::new(());
        let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        lock().clear();
        guard
    }

    #[test]
    fn create_then_open_roundtrips() {
        let _serial = reset();
        let id = create("Steve's Inventory".into(), 54, vec![None; 54]);
        assert_eq!(id.len(), 12);
        let (title, size, items) = open(&id).expect("fresh snapshot opens");
        assert_eq!(title, "Steve's Inventory");
        assert_eq!(size, 54);
        assert_eq!(items.len(), 54);
        // §2.11 — opening does **not** consume the entry; it stays live until
        // the TTL sweep or the capacity cap removes it.
        assert!(open(&id).is_some());
    }

    #[test]
    fn item_snapshot_centres_the_item() {
        let _serial = reset();
        // §2.10 — the item itself sits at index 13 of a 3×9 grid.
        let id = create_item_snapshot("Steve's Item - Diamond".into(), Vec::new());
        let (title, size, items) = open(&id).expect("item snapshot opens");
        assert_eq!(title, "Steve's Item - Diamond");
        assert_eq!(size, 27);
        assert_eq!(items.len(), 27);
        assert!(items[13].is_some(), "centre slot holds the item");
        assert!(items[12].is_none(), "slots before the centre stay empty");
    }

    #[test]
    fn item_snapshot_skips_the_cap() {
        let _serial = reset();
        // §2.10 — `createItemSnapshot` has no 100-entry trim, so creating more
        // than the cap leaves every id openable.
        let ids: Vec<String> = (0..(MAX_SNAPSHOTS + 5))
            .map(|i| create_item_snapshot(format!("t{i}"), Vec::new()))
            .collect();
        assert_eq!(len(), MAX_SNAPSHOTS + 5);
        assert!(open(&ids[0]).is_some(), "oldest item snapshot survived");
    }

    #[test]
    fn unknown_id_is_none() {
        let _serial = reset();
        assert!(open("000000000000").is_none());
    }

    #[test]
    fn overflow_evicts_oldest() {
        let _serial = reset();
        let mut ids = Vec::new();
        for i in 0..(MAX_SNAPSHOTS + 3) {
            ids.push(create(format!("t{i}"), 27, vec![None; 27]));
        }
        assert_eq!(len(), MAX_SNAPSHOTS);
        // The first three inserts were evicted; the survivors still open.
        assert!(open(&ids[0]).is_none());
        assert!(open(&ids[2]).is_none());
        assert!(open(&ids[3]).is_some());
    }

    #[test]
    fn expired_entries_are_swept() {
        let _serial = reset();
        let id = create("old".into(), 27, vec![None; 27]);
        // Backdate the entry past the TTL, then sweep.
        {
            let mut s = lock();
            if let Some(snap) = s.iter_mut().find(|snap| snap.id == id) {
                snap.created = Instant::now() - SNAPSHOT_TTL - Duration::from_secs(1);
            }
        }
        expire_snapshots();
        assert_eq!(len(), 0);
        assert!(open(&id).is_none());
    }
}
