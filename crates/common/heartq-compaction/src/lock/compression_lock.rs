use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusqlite::{params, Connection};

/// Error type for compression lock operations
#[derive(Debug, thiserror::Error)]
pub enum CompressionLockError {
    #[error("Lock already held by another process")]
    LockHeld,

    #[error("Lock expired")]
    LockExpired,

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Lock not acquired")]
    NotAcquired,
}

/// A compression lock to prevent concurrent compression on the same session.
///
/// This lock uses a database table to coordinate lock acquisition across
/// processes. The lock has a TTL to handle crashed holders.
pub struct CompressionLock {
    db_path: std::path::PathBuf,
    session_id: String,
    holder_id: String,
    ttl: Duration,
    acquired_at: Instant,
}

impl CompressionLock {
    /// Default TTL for locks (5 minutes)
    pub const DEFAULT_TTL: Duration = Duration::from_secs(300);

    /// Default refresh interval (TTL / 2)
    pub fn default_refresh_interval() -> Duration {
        Self::DEFAULT_TTL / 2
    }

    /// Try to acquire a compression lock for a session
    pub fn try_acquire(
        db: &Connection,
        session_id: &str,
        ttl_seconds: u64,
    ) -> Result<Option<Self>, CompressionLockError> {
        let holder_id = Self::generate_holder_id();
        let ttl = Duration::from_secs(ttl_seconds);
        let ttl_until = chrono::Utc::now() + chrono::Duration::seconds(ttl_seconds as i64);

        // Try to insert the lock
        let result = db.execute(
            "INSERT OR IGNORE INTO compression_locks (session_id, holder, ttl_until) VALUES (?1, ?2, ?3)",
            params![session_id, holder_id, ttl_until.to_rfc3339()],
        )?;

        if result == 0 {
            // Lock already exists, check if it's expired
            if let Ok(Some(existing)) = Self::get_lock_holder(db, session_id) {
                if existing.ttl_until < chrono::Utc::now() {
                    // Expired, try to take it
                    let updated = db.execute(
                        "UPDATE compression_locks SET holder = ?1, ttl_until = ?2 WHERE session_id = ?3 AND ttl_until < ?4",
                        params![holder_id, ttl_until.to_rfc3339(), session_id, chrono::Utc::now().to_rfc3339()],
                    )?;
                    if updated > 0 {
                        // Get the path from the database
                        let db_path = match db.path() {
                            Some(p) => std::path::PathBuf::from(p),
                            None => std::path::PathBuf::from(":memory:"),
                        };
                        
                        return Ok(Some(Self {
                            db_path,
                            session_id: session_id.to_string(),
                            holder_id,
                            ttl,
                            acquired_at: Instant::now(),
                        }));
                    }
                }
            }
            return Ok(None);
        }

        // Get the path from the database
        let db_path = match db.path() {
            Some(p) => std::path::PathBuf::from(p),
            None => std::path::PathBuf::from(":memory:"),
        };
        
        Ok(Some(Self {
            db_path,
            session_id: session_id.to_string(),
            holder_id,
            ttl,
            acquired_at: Instant::now(),
        }))
    }

    /// Refresh the lock TTL
    pub fn refresh(&self) -> Result<bool, CompressionLockError> {
        let conn = Connection::open(&self.db_path)?;
        let new_ttl = chrono::Utc::now() + chrono::Duration::seconds(self.ttl.as_secs() as i64);
        let updated = conn.execute(
            "UPDATE compression_locks SET ttl_until = ?1 WHERE session_id = ?2 AND holder = ?3",
            params![new_ttl.to_rfc3339(), self.session_id, self.holder_id],
        )?;
        Ok(updated > 0)
    }

    /// Release the lock
    pub fn release(self) -> Result<(), CompressionLockError> {
        let conn = Connection::open(&self.db_path)?;
        let released = conn.execute(
            "DELETE FROM compression_locks WHERE session_id = ?1 AND holder = ?2",
            params![self.session_id, self.holder_id],
        )?;

        if released == 0 {
            // Lock was already released or expired
            Ok(())
        } else {
            Ok(())
        }
    }

    /// Check if the lock is still valid (based on local TTL)
    pub fn is_valid(&self) -> bool {
        self.acquired_at.elapsed() < self.ttl
    }

    /// Get the holder ID
    pub fn holder_id(&self) -> &str {
        &self.holder_id
    }

    /// Get the session ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get the database path
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    fn generate_holder_id() -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        std::process::id().hash(&mut hasher);
        std::thread::current().id().hash(&mut hasher);
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default().as_nanos().hash(&mut hasher);

        format!("pid={}:{:x}", std::process::id(), hasher.finish())
    }

    fn get_lock_holder(db: &Connection, session_id: &str) -> Result<Option<LockInfo>, CompressionLockError> {
        let mut stmt = db.prepare(
            "SELECT holder, ttl_until FROM compression_locks WHERE session_id = ?1"
        )?;

        let mut rows = stmt.query(params![session_id])?;

        if let Some(row) = rows.next()? {
            let holder: String = row.get(0)?;
            let ttl_until: String = row.get(1)?;
            let ttl = chrono::DateTime::parse_from_rfc3339(&ttl_until)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());

            Ok(Some(LockInfo { holder, ttl_until: ttl }))
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug)]
struct LockInfo {
    holder: String,
    ttl_until: chrono::DateTime<chrono::Utc>,
}

/// Background lock refresher that spawns its own thread.
///
/// The refresher opens connections as needed since `rusqlite::Connection`
/// is not `Sync + Send`.
pub struct LockRefresher {
    db_path: std::path::PathBuf,
    session_id: String,
    holder_id: String,
    ttl: Duration,
    interval: Duration,
    stop_signal: Arc<AtomicU64>,
}

impl LockRefresher {
    /// Spawn a background thread to refresh the lock
    pub fn spawn(
        db_path: std::path::PathBuf,
        lock: CompressionLock,
        interval: Duration,
    ) -> Self {
        let stop_signal = Arc::new(AtomicU64::new(0));
        let stop_signal_clone = Arc::clone(&stop_signal);
        let db_path_clone = db_path.clone();
        let session_id = lock.session_id.clone();
        let holder_id = lock.holder_id.clone();
        let ttl = lock.ttl;

        std::thread::spawn(move || {
            let mut consecutive_failures = 0u32;

            loop {
                if stop_signal_clone.load(Ordering::Relaxed) != 0 {
                    break;
                }

                std::thread::sleep(interval);

                match Connection::open(&db_path_clone) {
                    Ok(conn) => {
                        let new_ttl = chrono::Utc::now() + chrono::Duration::seconds(ttl.as_secs() as i64);
                        match conn.execute(
                            "UPDATE compression_locks SET ttl_until = ?1 WHERE session_id = ?2 AND holder = ?3",
                            params![new_ttl.to_rfc3339(), session_id.clone(), holder_id.clone()],
                        ) {
                            Ok(updated) if updated > 0 => {
                                consecutive_failures = 0;
                            }
                            Ok(_) => {
                                // Lock was released or stolen
                                consecutive_failures += 1;
                                if consecutive_failures >= 2 {
                                    break;
                                }
                            }
                            Err(_) => {
                                consecutive_failures += 1;
                                if consecutive_failures >= 2 {
                                    break;
                                }
                            }
                        }
                    }
                    Err(_) => {
                        consecutive_failures += 1;
                        if consecutive_failures >= 2 {
                            break;
                        }
                    }
                }
            }
        });

        Self {
            db_path,
            session_id: lock.session_id,
            holder_id: lock.holder_id,
            ttl: lock.ttl,
            interval,
            stop_signal,
        }
    }

    /// Stop the refresher
    pub fn stop(&self) {
        self.stop_signal.store(1, Ordering::Relaxed);
    }
}

/// Create the compression_locks table if it doesn't exist
pub fn init_lock_table(db: &Connection) -> Result<(), CompressionLockError> {
    db.execute(
        "CREATE TABLE IF NOT EXISTS compression_locks (
            session_id TEXT PRIMARY KEY,
            holder TEXT NOT NULL,
            ttl_until TEXT NOT NULL
        )",
        [],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_generate_holder_id() {
        let id1 = CompressionLock::generate_holder_id();
        let id2 = CompressionLock::generate_holder_id();

        assert!(id1.starts_with("pid="));
        assert_ne!(id1, id2); // Should be unique
    }

    #[test]
    fn test_init_lock_table() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = Connection::open(&db_path).unwrap();

        init_lock_table(&conn).unwrap();

        // Verify table exists
        let count: i32 = conn.query_row(
            "SELECT COUNT(*) FROM compression_locks",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_lock_acquire_and_release() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = Connection::open(&db_path).unwrap();
        init_lock_table(&conn).unwrap();

        // Acquire lock
        let lock = CompressionLock::try_acquire(&conn, "session-1", 60).unwrap();
        assert!(lock.is_some());

        let lock = lock.unwrap();
        assert_eq!(lock.session_id(), "session-1");
        assert!(lock.is_valid());

        // Release lock
        lock.release().unwrap();

        // Can acquire again
        let lock2 = CompressionLock::try_acquire(&conn, "session-1", 60).unwrap();
        assert!(lock2.is_some());
    }

    #[test]
    fn test_lock_prevents_double_acquire() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = Connection::open(&db_path).unwrap();
        init_lock_table(&conn).unwrap();

        // First acquire succeeds
        let lock1 = CompressionLock::try_acquire(&conn, "session-1", 60).unwrap();
        assert!(lock1.is_some());

        // Second acquire fails
        let lock2 = CompressionLock::try_acquire(&conn, "session-1", 60).unwrap();
        assert!(lock2.is_none());
    }

    #[test]
    fn test_lock_refresh() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = Connection::open(&db_path).unwrap();
        init_lock_table(&conn).unwrap();

        // Acquire lock
        let lock = CompressionLock::try_acquire(&conn, "session-1", 60).unwrap();
        assert!(lock.is_some());
        let lock = lock.unwrap();

        // Refresh should succeed
        let refreshed = lock.refresh().unwrap();
        assert!(refreshed);

        // Release
        lock.release().unwrap();
    }
}
