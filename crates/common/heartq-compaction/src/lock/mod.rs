//! Lock module for preventing concurrent compression operations.

mod compression_lock;

pub use compression_lock::{
    init_lock_table, CompressionLock, CompressionLockError, LockRefresher,
};
