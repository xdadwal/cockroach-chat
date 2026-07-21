//! A persistent [`Store`] backed by SQLCipher: the entire database file is encrypted at rest with
//! a 256-bit key. That key is supplied by the caller (on device it comes from the platform
//! KeyVault — generated once, wrapped by the hardware keystore, never written to disk in the
//! clear). Destroying that key makes the whole database unrecoverable ciphertext, which is the
//! real cryptographic-erasure primitive behind panic wipe.
//!
//! Everything the mesh persists — channel history, peers, and store-and-forward envelopes — lives
//! inside the encrypted file, so a seized device yields no plaintext.

use meshcore::clock::Millis;
use meshcore::identity::Fingerprint;
use meshcore::store::{PeerRecord, Store, StoredMessage};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

pub struct SqliteStore {
    conn: Connection,
    path: PathBuf,
    history_max: usize,
    history_ms: Millis,
    envelope_ttl_ms: Millis,
    envelope_max_per_peer: usize,
}

impl SqliteStore {
    /// Open (creating if needed) an encrypted database at `path`, keyed with the raw 32-byte
    /// `key`. Retention parameters mirror [`meshcore::config::Tunables`].
    pub fn open(
        path: impl AsRef<Path>,
        key: &[u8; 32],
        history_max: usize,
        history_ms: Millis,
        envelope_ttl_ms: Millis,
        envelope_max_per_peer: usize,
    ) -> rusqlite::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(&path)?;
        // Raw-key mode: use the 32 bytes directly as the DB key (no passphrase KDF).
        conn.execute_batch(&format!("PRAGMA key = \"x'{}'\";", hex(key)))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS messages (
                 digest BLOB PRIMARY KEY,
                 channel TEXT NOT NULL,
                 sender BLOB NOT NULL,
                 timestamp_ms INTEGER NOT NULL,
                 body BLOB NOT NULL,
                 raw BLOB NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_messages_channel ON messages(channel, timestamp_ms);
             CREATE TABLE IF NOT EXISTS peers (
                 fingerprint BLOB PRIMARY KEY,
                 petname TEXT,
                 verified INTEGER NOT NULL,
                 last_eph BLOB NOT NULL,
                 last_seen_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS envelopes (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 recipient BLOB NOT NULL,
                 packet BLOB NOT NULL,
                 queued_ms INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_envelopes_recipient ON envelopes(recipient);",
        )?;
        Ok(Self {
            conn,
            path,
            history_max,
            history_ms,
            envelope_ttl_ms,
            envelope_max_per_peer,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn prune_channel(&self, channel: &str, now_ms: i64) {
        if self.history_ms > 0 {
            let _ = self.conn.execute(
                "DELETE FROM messages WHERE channel = ?1 AND ?2 - timestamp_ms >= ?3",
                params![channel, now_ms, self.history_ms as i64],
            );
        }
        // Keep only the newest `history_max` in this channel.
        let _ = self.conn.execute(
            "DELETE FROM messages WHERE channel = ?1 AND digest NOT IN (
                 SELECT digest FROM messages WHERE channel = ?1
                 ORDER BY timestamp_ms DESC LIMIT ?2
             )",
            params![channel, self.history_max as i64],
        );
    }
}

const MSG_COLS: &str = "digest, channel, sender, timestamp_ms, body, raw";

fn row_to_message(row: &rusqlite::Row) -> rusqlite::Result<StoredMessage> {
    Ok(StoredMessage {
        digest: to_arr8(row.get::<_, Vec<u8>>(0)?),
        channel: row.get(1)?,
        sender: to_arr8(row.get::<_, Vec<u8>>(2)?),
        timestamp_ms: row.get::<_, i64>(3)? as u64,
        body: row.get(4)?,
        raw: row.get(5)?,
    })
}

impl Store for SqliteStore {
    fn put_channel_message(&mut self, msg: StoredMessage) {
        let now = msg.timestamp_ms as i64;
        let _ = self.conn.execute(
            "INSERT OR IGNORE INTO messages (digest, channel, sender, timestamp_ms, body, raw)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &msg.digest[..],
                msg.channel,
                &msg.sender[..],
                msg.timestamp_ms as i64,
                msg.body,
                msg.raw
            ],
        );
        self.prune_channel(&msg.channel, now);
    }

    fn has_message(&self, digest: &[u8; 8]) -> bool {
        self.conn
            .query_row(
                "SELECT 1 FROM messages WHERE digest = ?1",
                params![&digest[..]],
                |_| Ok(()),
            )
            .optional()
            .unwrap_or(None)
            .is_some()
    }

    fn channel_history(&self, channel: &str, limit: usize) -> Vec<StoredMessage> {
        // Newest `limit`, returned oldest-first (matching the in-memory store).
        let sql = format!(
            "SELECT {MSG_COLS} FROM (
                 SELECT {MSG_COLS} FROM messages WHERE channel = ?1
                 ORDER BY timestamp_ms DESC LIMIT ?2
             ) ORDER BY timestamp_ms ASC"
        );
        let mut stmt = match self.conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let out = match stmt.query_map(params![channel, limit as i64], row_to_message) {
            Ok(iter) => iter.filter_map(Result::ok).collect(),
            Err(_) => Vec::new(),
        };
        out
    }

    fn channel_digests(&self, channel: &str) -> Vec<[u8; 8]> {
        let mut stmt = match self
            .conn
            .prepare("SELECT digest FROM messages WHERE channel = ?1")
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let out = match stmt.query_map(params![channel], |r| Ok(to_arr8(r.get::<_, Vec<u8>>(0)?))) {
            Ok(iter) => iter.filter_map(Result::ok).collect(),
            Err(_) => Vec::new(),
        };
        out
    }

    fn message_by_digest(&self, digest: &[u8; 8]) -> Option<StoredMessage> {
        self.conn
            .query_row(
                &format!("SELECT {MSG_COLS} FROM messages WHERE digest = ?1"),
                params![&digest[..]],
                row_to_message,
            )
            .optional()
            .unwrap_or(None)
    }

    fn upsert_peer(&mut self, peer: PeerRecord) {
        let _ = self.conn.execute(
            "INSERT OR REPLACE INTO peers (fingerprint, petname, verified, last_eph, last_seen_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                &peer.fingerprint[..],
                peer.petname,
                peer.verified as i64,
                &peer.last_eph[..],
                peer.last_seen_ms as i64
            ],
        );
    }

    fn get_peer(&self, fp: &Fingerprint) -> Option<PeerRecord> {
        self.conn
            .query_row(
                "SELECT fingerprint, petname, verified, last_eph, last_seen_ms
                 FROM peers WHERE fingerprint = ?1",
                params![&fp[..]],
                |row| {
                    Ok(PeerRecord {
                        fingerprint: to_arr32(row.get::<_, Vec<u8>>(0)?),
                        petname: row.get(1)?,
                        verified: row.get::<_, i64>(2)? != 0,
                        last_eph: to_arr8(row.get::<_, Vec<u8>>(3)?),
                        last_seen_ms: row.get::<_, i64>(4)? as u64,
                    })
                },
            )
            .optional()
            .unwrap_or(None)
    }

    fn queue_envelope(&mut self, recipient: Fingerprint, packet_bytes: Vec<u8>, now_ms: Millis) {
        let now = now_ms as i64;
        if self.envelope_ttl_ms > 0 {
            let _ = self.conn.execute(
                "DELETE FROM envelopes WHERE recipient = ?1 AND ?2 - queued_ms >= ?3",
                params![&recipient[..], now, self.envelope_ttl_ms as i64],
            );
        }
        let _ = self.conn.execute(
            "INSERT INTO envelopes (recipient, packet, queued_ms) VALUES (?1, ?2, ?3)",
            params![&recipient[..], packet_bytes, now],
        );
        // Keep only the newest `envelope_max_per_peer` for this recipient.
        let _ = self.conn.execute(
            "DELETE FROM envelopes WHERE recipient = ?1 AND id NOT IN (
                 SELECT id FROM envelopes WHERE recipient = ?1 ORDER BY id DESC LIMIT ?2
             )",
            params![&recipient[..], self.envelope_max_per_peer as i64],
        );
    }

    fn take_envelopes(&mut self, recipient: &Fingerprint) -> Vec<Vec<u8>> {
        let out = {
            let mut stmt = match self
                .conn
                .prepare("SELECT packet FROM envelopes WHERE recipient = ?1 ORDER BY id ASC")
            {
                Ok(s) => s,
                Err(_) => return Vec::new(),
            };
            let collected =
                match stmt.query_map(params![&recipient[..]], |r| r.get::<_, Vec<u8>>(0)) {
                    Ok(iter) => iter.filter_map(Result::ok).collect::<Vec<_>>(),
                    Err(_) => return Vec::new(),
                };
            collected
        };
        let _ = self.conn.execute(
            "DELETE FROM envelopes WHERE recipient = ?1",
            params![&recipient[..]],
        );
        out
    }

    fn panic_wipe(&mut self) {
        // Clear every row and reclaim (overwrite) the freed pages. The stronger guarantee is the
        // platform destroying the DB key, which leaves the file as unrecoverable ciphertext.
        let _ = self.conn.execute_batch(
            "DELETE FROM messages; DELETE FROM peers; DELETE FROM envelopes; VACUUM;",
        );
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn to_arr8(v: Vec<u8>) -> [u8; 8] {
    let mut a = [0u8; 8];
    let n = v.len().min(8);
    a[..n].copy_from_slice(&v[..n]);
    a
}

fn to_arr32(v: Vec<u8>) -> [u8; 32] {
    let mut a = [0u8; 32];
    let n = v.len().min(32);
    a[..n].copy_from_slice(&v[..n]);
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(d: u8, channel: &str, ts: u64) -> StoredMessage {
        StoredMessage {
            digest: [d; 8],
            channel: channel.to_string(),
            sender: [1; 8],
            timestamp_ms: ts,
            body: format!("body-{d}").into_bytes(),
            raw: vec![d; 4],
        }
    }

    fn open(dir: &std::path::Path, key: &[u8; 32]) -> SqliteStore {
        SqliteStore::open(
            dir.join("mesh.db"),
            key,
            1000,
            6 * 3_600_000,
            24 * 3_600_000,
            100,
        )
        .unwrap()
    }

    #[test]
    fn persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let key = [7u8; 32];
        {
            let mut s = open(dir.path(), &key);
            s.put_channel_message(msg(1, "#general", 100));
            s.upsert_peer(PeerRecord {
                fingerprint: [9; 32],
                petname: Some("ava".into()),
                verified: true,
                last_eph: [2; 8],
                last_seen_ms: 100,
            });
        } // close

        // Reopen with the same key — data must survive.
        let s = open(dir.path(), &key);
        assert!(s.has_message(&[1; 8]));
        assert_eq!(s.channel_history("#general", 10).len(), 1);
        let peer = s.get_peer(&[9; 32]).unwrap();
        assert_eq!(peer.petname.as_deref(), Some("ava"));
        assert!(peer.verified);
    }

    #[test]
    fn wrong_key_cannot_read() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut s = open(dir.path(), &[7u8; 32]);
            s.put_channel_message(msg(1, "#general", 100));
        }
        // Opening with a different key must fail (the file is encrypted).
        let bad = SqliteStore::open(dir.path().join("mesh.db"), &[8u8; 32], 1000, 0, 0, 100);
        let leaked = matches!(bad, Ok(s) if s.has_message(&[1; 8]));
        assert!(!leaked, "wrong key must not decrypt the database");
    }

    #[test]
    fn dedups_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = open(dir.path(), &[1u8; 32]);
        s.put_channel_message(msg(1, "#general", 100));
        s.put_channel_message(msg(1, "#general", 100)); // dup
        assert_eq!(s.channel_history("#general", 10).len(), 1);
        let m = s.message_by_digest(&[1; 8]).unwrap();
        assert_eq!(m.body, b"body-1");
        assert_eq!(m.raw, vec![1u8; 4]);
    }

    #[test]
    fn history_capped_by_count() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = SqliteStore::open(dir.path().join("m.db"), &[2u8; 32], 2, 0, 0, 100).unwrap();
        for i in 1..=5u8 {
            s.put_channel_message(msg(i, "#c", i as u64));
        }
        let h = s.channel_history("#c", 10);
        assert_eq!(h.len(), 2);
        assert_eq!(h[1].digest, [5; 8]); // newest kept
    }

    #[test]
    fn envelopes_queue_drain_and_expire() {
        let dir = tempfile::tempdir().unwrap();
        let mut s =
            SqliteStore::open(dir.path().join("e.db"), &[3u8; 32], 1000, 0, 1000, 100).unwrap();
        let fp = [5u8; 32];
        s.queue_envelope(fp, vec![1, 2, 3], 0);
        s.queue_envelope(fp, vec![4, 5, 6], 100);
        assert_eq!(s.take_envelopes(&fp).len(), 2);
        assert!(s.take_envelopes(&fp).is_empty());

        s.queue_envelope(fp, vec![1], 0);
        s.queue_envelope(fp, vec![2], 2000); // expires the first
        assert_eq!(s.take_envelopes(&fp), vec![vec![2u8]]);
    }

    #[test]
    fn panic_wipe_clears() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = open(dir.path(), &[4u8; 32]);
        s.put_channel_message(msg(1, "#general", 100));
        s.queue_envelope([9; 32], vec![1], 0);
        s.panic_wipe();
        assert!(!s.has_message(&[1; 8]));
        assert!(s.channel_history("#general", 10).is_empty());
        assert!(s.take_envelopes(&[9; 32]).is_empty());
    }
}
