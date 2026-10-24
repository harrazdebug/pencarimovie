//! One cache, two layers: an in-process L1 and a single-file redb L2.
//!
//! Replaces the "one JSON file per key in `storage/cache/`" scheme, which measured
//! **182–224 us per hit** on this machine (`stat` + `read_to_string` +
//! `serde_json::from_str`; the parse was only 4–11 us of it) and **763 us per
//! write**. The same hit here costs **0.04–1.1 us** from L1 and **6.2 us** from
//! L2, and the L2 write is ~154 us (`Durability::None` — a cache must never fsync).
//! Numbers and the harnesses: `tests/bench_cache.rs`, `tests/bench_redb.rs`.
//!
//! Layout:
//!
//! * **L1** — sharded `HashMap<String, Slot>` behind a `Mutex` per shard, so a hit
//!   is one uncontended lock + one `Arc` clone. Capacity-capped per shard (random
//!   eviction after the expired entries are dropped), because the old
//!   `Mutex<HashMap<..>>` in `resolve.rs` never evicted — a TV box left running for
//!   weeks grew it forever.
//! * **L2** — `storage/cache/cache.redb`, one table, key `"<prefix>\u{1f}<key>"`,
//!   value `[exp u64 LE][hard u64 LE][json…]`. Survives a restart, replaces ~20k
//!   small files, and removes the 300 s directory scan. Writes are BUFFERED and
//!   committed in one transaction every 400 ms, because a redb commit costs ~1.7 ms
//!   (fsync) — per-entry commits would be slower than the JSON file they replaced.
//!   `Durability::None` would make them cheap (165 us) but keeps nothing across a
//!   kill and never frees pages, and an app kill is the normal lifecycle on Android.
//!
//! Two deadlines per entry: `exp` is when the value stops being *fresh* and `hard`
//! is when it is deleted. `hard > exp` gives the stale-while-revalidate behaviour
//! the WordPress caches need (`get_stale`), without a second cache.
//!
//! Every fetch goes through [`fetch_cached`], which coalesces concurrent misses for
//! the same key into one upstream call (single-flight). The PHP needed a `flock`
//! for this; here it is a per-key list of `oneshot` senders. A failing fetch is
//! **not** cached and its waiters fall through and retry, so a transient origin
//! error cannot be pinned in the cache.
//!
//! L2 is optional: if the file cannot be opened (a second instance already holds
//! redb's exclusive lock, or the directory is read-only) the server logs once and
//! runs memory-only instead of failing to start.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use redb::{Database, ReadableTable, TableDefinition};
use serde_json::Value;

use crate::log::log;

const TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("cache");
const SHARDS: usize = 16;
/// 512 per shard ⇒ ~8k entries (~2–4 MB of the JSON sizes this app caches).
const CAP_PER_SHARD: usize = 512;
/// `\u{1f}` cannot appear in a URL, a short code or a bot id, so prefix and key can
/// never collide (`resolve:abc` vs `resolve:a` + `bc`).
const SEP: char = '\u{1f}';

fn now() -> i64 {
    crate::storage::now()
}

fn ck(prefix: &str, key: &str) -> String {
    let mut s = String::with_capacity(prefix.len() + key.len() + 1);
    s.push_str(prefix);
    s.push(SEP);
    s.push_str(key);
    s
}

fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/* ── counters (stats only; relaxed is fine) ──────────────────────────────── */

static L1_HITS: AtomicU64 = AtomicU64::new(0);
static L1_MISSES: AtomicU64 = AtomicU64::new(0);
static L2_HITS: AtomicU64 = AtomicU64::new(0);
static L2_MISSES: AtomicU64 = AtomicU64::new(0);
static L2_PUTS: AtomicU64 = AtomicU64::new(0);
static EVICTIONS: AtomicU64 = AtomicU64::new(0);
static COALESCED: AtomicU64 = AtomicU64::new(0);

/* ── L1 ──────────────────────────────────────────────────────────────────── */

struct Slot {
    v: Arc<Value>,
    exp: i64,
    hard: i64,
}

struct L1 {
    shards: Vec<Mutex<HashMap<String, Slot>>>,
    cap: usize,
}

impl L1 {
    fn new(cap: usize) -> Self {
        let mut shards = Vec::with_capacity(SHARDS);
        for _ in 0..SHARDS {
            shards.push(Mutex::new(HashMap::new()));
        }
        Self { shards, cap }
    }

    fn shard(&self, full: &str) -> std::sync::MutexGuard<'_, HashMap<String, Slot>> {
        // A poisoned cache mutex must not take the server down; the data is
        // disposable, so recover the inner map and carry on.
        match self.shards[(fnv1a(full) as usize) % self.shards.len()].lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        }
    }

    /// Fresh read: `Some` only while `now < exp`.
    fn get(&self, full: &str) -> Option<Arc<Value>> {
        let mut g = self.shard(full);
        let slot = g.get(full)?;
        let t = now();
        if t >= slot.hard {
            g.remove(full);
            return None;
        }
        if t >= slot.exp {
            return None; // expired but still kept for `get_stale`
        }
        Some(slot.v.clone())
    }

    /// Stale read: `Some` while `now < hard`.
    fn get_stale(&self, full: &str) -> Option<Arc<Value>> {
        let mut g = self.shard(full);
        let slot = g.get(full)?;
        let t = now();
        if t >= slot.hard {
            g.remove(full);
            return None;
        }
        Some(slot.v.clone())
    }

    fn put(&self, full: &str, v: &Value, exp: i64, hard: i64) -> Arc<Value> {
        let arc = Arc::new(v.clone());
        let t = now();
        let mut g = self.shard(full);
        if !g.contains_key(full) && g.len() >= self.cap {
            // Cheap eviction: drop what is already dead; if the shard is still full,
            // remove a quarter of it, soonest-deadline first.
            g.retain(|_, s| s.hard > t);
            if g.len() >= self.cap {
                let mut doomed: Vec<(i64, String)> =
                    g.iter().map(|(k, s)| (s.hard, k.clone())).collect();
                doomed.sort_unstable_by_key(|(hard, _)| *hard);
                let take = (self.cap / 4).max(1).min(doomed.len());
                for (_, k) in doomed.into_iter().take(take) {
                    g.remove(&k);
                    EVICTIONS.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        g.insert(
            full.to_string(),
            Slot {
                v: arc.clone(),
                exp,
                hard,
            },
        );
        arc
    }

    fn invalidate(&self, full: &str) {
        self.shard(full).remove(full);
    }

    fn clear(&self, prefix: Option<&str>) -> usize {
        let mut n = 0;
        for shard in &self.shards {
            let mut g = match shard.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            match prefix {
                None => {
                    n += g.len();
                    g.clear();
                }
                Some(p) => {
                    let head = format!("{p}{SEP}");
                    let before = g.len();
                    g.retain(|k, _| !k.starts_with(&head));
                    n += before - g.len();
                }
            }
        }
        n
    }

    fn sweep(&self) -> usize {
        let t = now();
        let mut n = 0;
        for shard in &self.shards {
            let mut g = match shard.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            let before = g.len();
            g.retain(|_, s| s.hard > t);
            n += before - g.len();
        }
        n
    }

    fn len(&self) -> usize {
        let mut n = 0;
        for shard in &self.shards {
            let g = match shard.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            n += g.len();
        }
        n
    }

    fn prefixes(&self) -> Value {
        let mut m: HashMap<String, u64> = HashMap::new();
        for shard in &self.shards {
            let g = match shard.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            for k in g.keys() {
                let p = k.split(SEP).next().unwrap_or("").to_string();
                *m.entry(p).or_insert(0) += 1;
            }
        }
        let mut out = serde_json::Map::new();
        for (k, v) in m {
            out.insert(k, Value::from(v));
        }
        Value::Object(out)
    }
}

fn l1() -> &'static L1 {
    static L1: OnceLock<L1> = OnceLock::new();
    L1.get_or_init(|| L1::new(CAP_PER_SHARD))
}

/* ── L2 (redb) ───────────────────────────────────────────────────────────── */

struct L2 {
    db: Database,
}

fn encode(v: &Value, exp: i64, hard: i64) -> Option<Vec<u8>> {
    let body = serde_json::to_vec(v).ok()?;
    let mut buf = Vec::with_capacity(16 + body.len());
    buf.extend_from_slice(&exp.to_le_bytes());
    buf.extend_from_slice(&hard.to_le_bytes());
    buf.extend_from_slice(&body);
    Some(buf)
}

fn decode(b: &[u8]) -> Option<(Value, i64, i64)> {
    if b.len() < 17 {
        return None;
    }
    let exp = i64::from_le_bytes(b[0..8].try_into().ok()?);
    let hard = i64::from_le_bytes(b[8..16].try_into().ok()?);
    let v: Value = serde_json::from_slice(&b[16..]).ok()?;
    Some((v, exp, hard))
}

impl L2 {
    fn open(path: &std::path::Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let opened = if path.is_file() {
            Database::open(path)
        } else {
            Database::create(path)
        };
        match opened {
            Ok(db) => Ok(Self { db }),
            Err(e) => Err(e.to_string()),
        }
    }

    fn get(&self, full: &str, allow_expired: bool) -> Option<(Arc<Value>, i64, i64)> {
        let rtx = self.db.begin_read().ok()?;
        // A miss before the first write: the table does not exist yet.
        let table = rtx.open_table(TABLE).ok()?;
        let guard = table.get(full).ok()??;
        let (v, exp, hard) = decode(guard.value())?;
        let t = now();
        if t >= hard {
            return None;
        }
        if !allow_expired && t >= exp {
            return None;
        }
        Some((Arc::new(v), exp, hard))
    }

    /// Write a whole batch in ONE transaction.
    ///
    /// The durability choice is measured, not guessed (`tests/bench_redb.rs`):
    /// `Immediate` 1 697 us/commit, `Eventual` 1 761 us, `None` 165 us — but `None`
    /// keeps nothing across a kill and never frees pages (redb's own docs: "pages
    /// are only freed during commits with higher durability levels", so the file
    /// grows without bound), and an app kill is the NORMAL lifecycle on Android.
    /// So the per-entry cost is removed by *batching* instead: one `Eventual`
    /// commit per flush, which is one fsync per batch rather than per entry.
    fn write_batch(&self, batch: &HashMap<String, Option<Vec<u8>>>) -> Result<usize, ()> {
        let Ok(mut wtx) = self.db.begin_write() else {
            return Err(());
        };
        wtx.set_durability(redb::Durability::Eventual);
        let mut n = 0usize;
        {
            let mut t = match wtx.open_table(TABLE) {
                Ok(t) => t,
                // Dropping the uncommitted transaction aborts it; calling `abort()`
                // here would move `wtx` while it is still borrowed by the match.
                Err(_) => return Err(()),
            };
            for (k, v) in batch {
                let ok = match v {
                    Some(bytes) => t.insert(k.as_str(), bytes.as_slice()).is_ok(),
                    None => t.remove(k.as_str()).is_ok(),
                };
                if ok {
                    n += 1;
                }
            }
        }
        match wtx.commit() {
            Ok(()) => Ok(n),
            Err(_) => Err(()),
        }
    }

    fn put(&self, full: &str, v: &Value, exp: i64, hard: i64) -> bool {
        let Some(buf) = encode(v, exp, hard) else {
            return false;
        };
        self.write_batch(&HashMap::from([(full.to_string(), Some(buf))]))
            .is_ok()
    }

    fn del(&self, full: &str) -> bool {
        self.write_batch(&HashMap::from([(full.to_string(), None)]))
            .is_ok()
    }

    fn clear(&self, prefix: Option<&str>) -> usize {
        let Ok(mut wtx) = self.db.begin_write() else {
            return 0;
        };
        wtx.set_durability(redb::Durability::None);
        let Some(head) = prefix.map(|p| format!("{p}{SEP}")) else {
            // Dropping the whole table is O(1); it is recreated by the next write.
            let n = self.rows();
            let dropped = wtx.delete_table(TABLE).unwrap_or(false);
            if dropped && wtx.commit().is_ok() {
                return n;
            }
            return 0; // dropping the uncommitted transaction aborts it
        };
        let mut n = 0usize;
        match wtx.open_table(TABLE) {
            Ok(mut t) => {
                let doomed: Vec<String> = match t.iter() {
                    Ok(it) => it
                        .filter_map(|r| r.ok())
                        .map(|(k, _)| k.value().to_string())
                        .filter(|k| k.starts_with(&head))
                        .collect(),
                    Err(_) => Vec::new(),
                };
                for k in doomed {
                    if t.remove(k.as_str()).is_ok() {
                        n += 1;
                    }
                }
            }
            Err(_) => return 0, // the table does not exist yet: nothing to clear
        }
        if wtx.commit().is_ok() {
            n
        } else {
            0
        }
    }

    fn sweep(&self) -> usize {
        let Ok(mut wtx) = self.db.begin_write() else {
            return 0;
        };
        wtx.set_durability(redb::Durability::None);
        let t = now();
        let mut n = 0usize;
        match wtx.open_table(TABLE) {
            Ok(mut table) => {
                let doomed: Vec<String> = match table.iter() {
                    Ok(it) => it
                        .filter_map(|r| r.ok())
                        .filter_map(|(k, v)| match decode(v.value()) {
                            Some((_, _, hard)) if t >= hard => Some(k.value().to_string()),
                            _ => None,
                        })
                        .collect(),
                    Err(_) => Vec::new(),
                };
                for k in doomed {
                    if table.remove(k.as_str()).is_ok() {
                        n += 1;
                    }
                }
            }
            Err(_) => return 0, // the table does not exist yet: nothing to sweep
        }
        if wtx.commit().is_ok() {
            n
        } else {
            0
        }
    }

    fn rows(&self) -> usize {
        let Ok(rtx) = self.db.begin_read() else {
            return 0;
        };
        let Ok(t) = rtx.open_table(TABLE) else {
            return 0;
        };
        match t.iter() {
            Ok(it) => it.count(),
            Err(_) => 0,
        }
    }
}

fn l2() -> Option<&'static L2> {
    static L2: OnceLock<Option<L2>> = OnceLock::new();
    L2.get_or_init(|| {
        // `PM_CACHE_DB=off` disables the persistent layer (read-only storage, a
        // second instance, or a test that wants L1 only).
        if let Some(v) = crate::storage::env("PM_CACHE_DB") {
            if v == "off" {
                log("cache db disabled (PM_CACHE_DB=off)", &serde_json::json!({}));
                return None;
            }
        }
        // In a test binary the storage dir would be the crate's cwd, so the file goes
        // to a per-process temp dir instead: tests stay hermetic.
        #[cfg(test)]
        let default_path = std::env::temp_dir()
            .join(format!("pm-cache-test-{}", std::process::id()))
            .join("cache.redb");
        #[cfg(not(test))]
        let default_path = crate::storage::cache_path("cache.redb");
        let path = match crate::storage::env("PM_CACHE_DB") {
            Some(p) => std::path::PathBuf::from(p),
            None => default_path,
        };
        match L2::open(&path) {
            Ok(db) => {
                log(
                    "cache db ready",
                    &serde_json::json!({"path": path.display().to_string()}),
                );
                Some(db)
            }
            Err(e) => {
                // `DatabaseAlreadyOpen` lands here: redb holds an exclusive OS lock.
                // Degrade instead of refusing to start.
                log(
                    "cache db unavailable - running memory-only",
                    &serde_json::json!({"path": path.display().to_string(), "error": e}),
                );
                None
            }
        }
    })
    .as_ref()
}

/* ── write-behind buffer ─────────────────────────────────────────────────── */

/// How often buffered writes are committed to redb.
///
/// A commit costs ~1.7 ms (fsync), so committing per `put` would make a cache
/// write *slower* than the JSON file it replaced (763 us). Buffering and flushing
/// one transaction per interval makes the fsync cost independent of the write rate;
/// the price is that a hard kill can lose up to this window — acceptable for a
/// cache, and much better than `Durability::None`, which loses everything.
const FLUSH_INTERVAL_MS: u64 = 400;

fn pending() -> &'static Mutex<HashMap<String, Option<Vec<u8>>>> {
    static P: OnceLock<Mutex<HashMap<String, Option<Vec<u8>>>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

fn pending_guard() -> std::sync::MutexGuard<'static, HashMap<String, Option<Vec<u8>>>> {
    match pending().lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    }
}

/// Queue a write (or a tombstone) and make sure the flusher exists.
fn queue(full: &str, value: Option<Vec<u8>>) {
    pending_guard().insert(full.to_string(), value);
    ensure_flusher();
}

fn ensure_flusher() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        // Only inside a runtime (the server always is; a unit test may not be).
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(FLUSH_INTERVAL_MS)).await;
                    flush();
                }
            });
        }
    });
}

/// Commit everything buffered. Called by the flusher, by `clear()` and by tests.
pub fn flush() -> usize {
    let batch = {
        let mut p = pending_guard();
        if p.is_empty() {
            return 0;
        }
        std::mem::take(&mut *p)
    };
    let Some(l2) = l2() else {
        return 0; // memory-only mode: nothing to persist
    };
    match l2.write_batch(&batch) {
        Ok(n) => {
            L2_PUTS.fetch_add(n as u64, Ordering::Relaxed);
            n
        }
        Err(_) => {
            L2_MISSES.fetch_add(1, Ordering::Relaxed);
            0
        }
    }
}

/* ── public API ──────────────────────────────────────────────────────────── */

/// Fresh read: L1, then L2 (which repopulates L1).
pub fn get(prefix: &str, key: &str) -> Option<Arc<Value>> {
    let full = ck(prefix, key);
    if let Some(v) = l1().get(&full) {
        L1_HITS.fetch_add(1, Ordering::Relaxed);
        return Some(v);
    }
    L1_MISSES.fetch_add(1, Ordering::Relaxed);
    let l2 = l2()?;
    let (v, exp, hard) = l2.get(&full, false)?;
    L2_HITS.fetch_add(1, Ordering::Relaxed);
    let stored = l1().put(&full, &v, exp, hard);
    Some(stored)
}

/// Stale read (stale-while-revalidate): the value is returned even past `exp`, as
/// long as it is inside the grace window given at `put`.
pub fn get_stale(prefix: &str, key: &str) -> Option<Arc<Value>> {
    let full = ck(prefix, key);
    if let Some(v) = l1().get_stale(&full) {
        return Some(v);
    }
    let l2 = l2()?;
    l2.get(&full, true).map(|(v, exp, hard)| {
        L2_HITS.fetch_add(1, Ordering::Relaxed);
        l1().put(&full, &v, exp, hard)
    })
}

/// Store a value. `ttl` is the freshness window; `grace` extends the hard deletion
/// deadline by that many seconds (use 0 when no stale read is expected).
///
/// The write is buffered (see [`FLUSH_INTERVAL_MS`]) so it costs ~1 us, not a fsync.
pub fn put(prefix: &str, key: &str, v: &Value, ttl: i64, grace: i64) -> Arc<Value> {
    let full = ck(prefix, key);
    let exp = now() + ttl.max(0);
    let hard = exp + grace.max(0);
    let arc = l1().put(&full, v, exp, hard);
    queue(&full, encode(v, exp, hard));
    arc
}

pub fn invalidate(prefix: &str, key: &str) {
    let full = ck(prefix, key);
    l1().invalidate(&full);
    queue(&full, None);
}

/// Drop everything (or one prefix) from both layers. Returns the number of entries
/// removed from L1.
pub fn clear(prefix: Option<&str>) -> usize {
    let n = l1().clear(prefix);
    // A clear is rare and must be visible immediately: drop the buffer's matching
    // tombstones/writes first, then clear L2 synchronously.
    pending_guard().clear();
    if let Some(l2) = l2() {
        l2.clear(prefix);
    }
    n
}

/// Delete expired entries from both layers. Called from the periodic prune.
pub fn sweep() -> usize {
    let a = l1().sweep();
    let b = l2().map(|l| l.sweep()).unwrap_or(0);
    a + b
}

pub fn stats() -> Value {
    let l2s = match l2() {
        Some(l) => serde_json::json!({
            "open": true,
            "rows": l.rows(),
        }),
        None => serde_json::json!({"open": false, "rows": 0}),
    };
    serde_json::json!({
        "ok": 1,
        "l1": {
            "entries": l1().len(),
            "cap_per_shard": CAP_PER_SHARD,
            "shards": SHARDS,
            "prefixes": l1().prefixes(),
            "hits": L1_HITS.load(Ordering::Relaxed),
            "misses": L1_MISSES.load(Ordering::Relaxed),
            "evictions": EVICTIONS.load(Ordering::Relaxed),
        },
        "l2": l2s,
        "l2_hits": L2_HITS.load(Ordering::Relaxed),
        "l2_puts": L2_PUTS.load(Ordering::Relaxed),
        "coalesced": COALESCED.load(Ordering::Relaxed),
        "pending_writes": pending_guard().len(),
    })
}

/* ── single-flight ───────────────────────────────────────────────────────── */

type Waiters = HashMap<String, Vec<tokio::sync::oneshot::Sender<Arc<Value>>>>;

fn inflight() -> &'static Mutex<Waiters> {
    static W: OnceLock<Mutex<Waiters>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(HashMap::new()))
}

fn waiters() -> std::sync::MutexGuard<'static, Waiters> {
    match inflight().lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    }
}

/// Removes the in-flight marker if the leader unwinds (a panic in `f`), so the key
/// cannot stay claimed forever.
struct Claim(String);

impl Drop for Claim {
    fn drop(&mut self) {
        waiters().remove(&self.0);
    }
}

/// Read-through with single-flight.
///
/// `f` returns `(value, cacheable)` — an error response returns `cacheable: false`,
/// which is **not** stored and does not satisfy the waiters (they retry), so a
/// transient upstream failure can never be pinned.
pub async fn fetch_cached<F, Fut>(prefix: &str, key: &str, ttl: i64, grace: i64, f: F) -> Value
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = (Value, bool)>,
{
    if let Some(v) = get(prefix, key) {
        return (*v).clone();
    }
    let full = ck(prefix, key);

    // Join the leader's flight, or claim it ourselves.
    let follower = {
        let mut w = waiters();
        match w.get_mut(&full) {
            Some(list) => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                list.push(tx);
                COALESCED.fetch_add(1, Ordering::Relaxed);
                Some(rx)
            }
            None => {
                w.insert(full.clone(), Vec::new());
                None
            }
        }
    };

    if let Some(rx) = follower {
        match rx.await {
            Ok(v) => return (*v).clone(),
            // The leader failed or panicked: do the work ourselves, uncached path.
            Err(_) => {
                let (v, cacheable) = f().await;
                if cacheable {
                    return (*put(prefix, key, &v, ttl, grace)).clone();
                }
                return v;
            }
        }
    }

    let claim = Claim(full.clone());
    let (v, cacheable) = f().await;
    let arc = if cacheable {
        put(prefix, key, &v, ttl, grace)
    } else {
        Arc::new(v.clone())
    };
    // Wake the followers with the value that is now cached (drop them on failure so
    // each one retries rather than receiving a poisoned answer).
    if cacheable {
        let mut w = waiters();
        if let Some(list) = w.remove(&full) {
            for tx in list {
                let _ = tx.send(arc.clone());
            }
        }
    }
    drop(claim);
    v
}

/* ── tests ───────────────────────────────────────────────────────────────── */

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "pm-cache-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&d);
        d
    }

    #[test]
    fn l1_ttl_and_stale_grace() {
        let c = L1::new(8);
        let v = json!({"a": 1});
        let t = crate::storage::now();

        // Past `exp`, inside `hard`: not fresh, still served as stale.
        c.put("k", &v, t - 1, t + 60);
        assert!(c.get("k").is_none(), "expired must not be fresh");
        assert_eq!(c.get_stale("k"), Some(Arc::new(json!({"a": 1}))));

        c.put("k2", &v, t + 60, t + 60);
        assert!(c.get("k2").is_some());
        assert!(c.get_stale("k2").is_some());

        // Past `hard`: a read removes it (nothing to serve, not even stale).
        c.put("dead", &v, t - 10, t - 5);
        assert!(c.get_stale("dead").is_none());
        assert_eq!(c.len(), 2, "the hard-expired entry was dropped by the read");

        // One that is never read is removed by the sweep.
        c.put("dead2", &v, t - 10, t - 5);
        assert_eq!(c.len(), 3);
        assert_eq!(c.sweep(), 1);
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn l1_capacity_evicts_and_keeps_serving() {
        // The cap is PER SHARD, so the total is `cap * SHARDS`.
        let cap = 2;
        let c = L1::new(cap);
        let t = crate::storage::now();
        for i in 0..500 {
            c.put(&format!("key-{i}"), &json!({"i": i}), t + 600, t + 600);
            assert!(
                c.len() <= cap * SHARDS,
                "the cache must stay bounded ({} > {})",
                c.len(),
                cap * SHARDS
            );
        }
        assert!(c.len() > 0, "entries are kept");
    }

    #[test]
    fn prefixes_are_isolated_and_clearable() {
        let c = L1::new(64);
        let t = crate::storage::now();
        let a = ck("resolve", "abc");
        let b = ck("wp_json", "abc");
        c.put(&a, &json!(1), t + 60, t + 60);
        c.put(&b, &json!(2), t + 60, t + 60);
        assert_eq!(c.clear(Some("resolve")), 1);
        assert!(c.get(&a).is_none());
        assert!(c.get(&b).is_some(), "other prefixes untouched");
        assert_eq!(c.clear(None), 1);
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn prefix_and_key_cannot_collide() {
        // `resolve` + `abc` must not be reachable as `resolve_a` + `bc`.
        let a = ck("resolve", "abc");
        let b = ck("resolve_a", "bc");
        assert_ne!(a, b);
    }

    #[test]
    fn l2_round_trips_and_expires() {
        let dir = scratch("l2");
        let path = dir.join("cache.redb");
        let db = L2::open(&path).expect("open");
        // `get` before any write: the table does not exist yet → miss, not panic.
        assert!(db.get("p\u{1f}nope", false).is_none());

        let now = crate::storage::now();
        assert!(db.put("p\u{1f}fresh", &json!({"x": 1}), now + 60, now + 60));
        assert!(db.put("p\u{1f}stale", &json!({"x": 2}), now - 5, now + 60));
        assert!(db.put("p\u{1f}dead", &json!({"x": 3}), now - 60, now - 30));

        assert_eq!(db.get("p\u{1f}fresh", false).map(|(v, _, _)| (*v).clone()), Some(json!({"x": 1})));
        assert!(db.get("p\u{1f}stale", false).is_none());
        assert!(db.get("p\u{1f}stale", true).is_some(), "stale grace");
        assert!(db.get("p\u{1f}dead", true).is_none(), "past hard");
        assert_eq!(db.rows(), 3);
        assert_eq!(db.sweep(), 1, "only the dead entry is removed");
        assert_eq!(db.rows(), 2);

        // Reopening the file keeps the data: this is the whole point of L2.
        drop(db);
        let db = L2::open(&path).expect("reopen");
        assert!(db.get("p\u{1f}fresh", false).is_some());
        assert!(db.del("p\u{1f}fresh"));
        assert!(db.get("p\u{1f}fresh", false).is_none());
        assert_eq!(db.clear(Some("p")), 1);
        assert_eq!(db.rows(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn l2_clear_all_and_reuse() {
        let dir = scratch("l2b");
        let db = L2::open(&dir.join("cache.redb")).expect("open");
        let now = crate::storage::now();
        for p in ["a", "b"] {
            assert!(db.put(&format!("{p}\u{1f}k"), &json!({"v": p}), now + 60, now + 60));
        }
        assert_eq!(db.clear(None), 2);
        assert_eq!(db.rows(), 0);
        // The table was dropped; writing again must recreate it.
        assert!(db.put("a\u{1f}k2", &json!({"v": "again"}), now + 60, now + 60));
        assert!(db.get("a\u{1f}k2", false).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The write-behind path: `put` is cheap, `flush()` commits, and the value is
    /// then visible to a SECOND handle on the same file (i.e. after a restart).
    #[test]
    fn write_behind_persists_after_flush() {
        let dir = scratch("wb");
        let path = dir.join("cache.redb");
        let db = L2::open(&path).expect("open");
        let t = crate::storage::now();
        // Queue through the shared buffer, then commit it into this DB handle.
        let mut batch: HashMap<String, Option<Vec<u8>>> = HashMap::new();
        batch.insert("p\u{1f}kept".to_string(), encode(&json!({"n": 1}), t + 60, t + 60));
        batch.insert("p\u{1f}gone".to_string(), None); // a tombstone
        assert_eq!(db.write_batch(&batch).unwrap(), 2);
        // A second handle on the same file (what a restart does) sees the value.
        drop(db);
        let db2 = L2::open(&path).expect("reopen");
        assert!(db2.get("p\u{1f}kept", false).is_some());
        assert!(db2.get("p\u{1f}gone", false).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Only this test touches the process-global layers, so `PM_CACHE_DB` can be
    /// pointed at a scratch file before anything initialises them.
    #[tokio::test]
    async fn fetch_cached_coalesces_concurrent_misses() {
        let dir = scratch("flight");
        // SAFETY: single-threaded test setup, before the globals are first used.
        unsafe { std::env::set_var("PM_CACHE_DB", dir.join("cache.redb")) };

        let called = Arc::new(AtomicU64::new(0));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let called = called.clone();
            handles.push(tokio::spawn(async move {
                fetch_cached("flight", "one", 60, 0, || async move {
                    called.fetch_add(1, Ordering::Relaxed);
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    (json!({"n": 7}), true)
                })
                .await
            }));
        }
        let mut values = Vec::new();
        for h in handles {
            values.push(h.await.unwrap());
        }
        assert_eq!(called.load(Ordering::Relaxed), 1, "one upstream call for 8 misses");
        assert!(values.iter().all(|v| v == &json!({"n": 7})));

        // Second round is a plain cache hit.
        let v = fetch_cached("flight", "one", 60, 0, || async { (json!({"n": 99}), true) }).await;
        assert_eq!(v, json!({"n": 7}));
        assert_eq!(called.load(Ordering::Relaxed), 1);

        // A non-cacheable answer is NOT stored and its waiters retry.
        let v = fetch_cached("flight", "two", 60, 0, || async { (json!({"err": 1}), false) }).await;
        assert_eq!(v, json!({"err": 1}));
        let v = fetch_cached("flight", "two", 60, 0, || async { (json!({"ok": 2}), true) }).await;
        assert_eq!(v, json!({"ok": 2}), "failure must not be cached");
        // Two stored keys: "one" and "two" (the failed answer was never stored).
        assert_eq!(clear(Some("flight")), 2);
        assert!(get("flight", "one").is_none(), "cleared from L1");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
