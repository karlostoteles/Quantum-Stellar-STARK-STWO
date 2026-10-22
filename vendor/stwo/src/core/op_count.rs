//! stwo-soroban patch (see PATCHES.md): counts field operations and Blake2s hashing, behind the
//! `op-count` feature. Off by default; the counters and checkpoints compile to nothing otherwise.
//!
//! The counts are of the operations as written in the source (an `M31 * M31` is one M31 multiply;
//! a `QM31 * QM31` is one QM31 multiply *and* the M31 operations it expands to), so the M31 totals
//! are the number of base-field operations actually executed and the extension-field totals say
//! how they were grouped.
use core::sync::atomic::{AtomicU64, Ordering::Relaxed};

use std_shims::Vec;

macro_rules! counters {
    ($($name:ident),* $(,)?) => {
        $(pub static $name: AtomicU64 = AtomicU64::new(0);)*
        pub const NAMES: &[&str] = &[$(stringify!($name)),*];
        pub fn snapshot() -> Vec<u64> {
            [$($name.load(Relaxed)),*].to_vec()
        }
        pub fn reset() {
            $($name.store(0, Relaxed);)*
        }
    };
}

counters!(
    M31_ADD, M31_SUB, M31_NEG, M31_MUL, M31_INV,
    CM31_MUL, CM31_INV,
    QM31_MUL, QM31_MUL_CM31, QM31_INV,
    BLAKE2S_HASHES, BLAKE2S_BYTES, BLAKE2S_COMPRESSIONS,
);

/// Records that a Blake2s hash of `len` bytes was finalized.
pub fn blake2s_finalized(len: usize) {
    BLAKE2S_HASHES.fetch_add(1, Relaxed);
    BLAKE2S_BYTES.fetch_add(len as u64, Relaxed);
    // Blake2s compresses one 64-byte block per call, the last one (possibly empty) at finalize.
    BLAKE2S_COMPRESSIONS.fetch_add(core::cmp::max(1, len.div_ceil(64)) as u64, Relaxed);
}

#[cfg(feature = "std")]
mod checkpoints {
    use std::sync::Mutex;
    use std::vec::Vec;

    static CHECKPOINTS: Mutex<Vec<(&'static str, Vec<u64>)>> = Mutex::new(Vec::new());
    static LAST: Mutex<Option<Vec<u64>>> = Mutex::new(None);

    /// Closes a phase: records the counts since the previous checkpoint (or since `start`) under
    /// `name`.
    pub fn checkpoint(name: &'static str) {
        let now = super::snapshot();
        let mut last = LAST.lock().unwrap();
        let base = last.clone().unwrap_or_else(|| std::vec![0; now.len()]);
        let delta = now.iter().zip(&base).map(|(a, b)| a - b).collect();
        CHECKPOINTS.lock().unwrap().push((name, delta));
        *last = Some(now);
    }

    /// Starts a measurement: clears the counters and the recorded phases.
    pub fn start() {
        super::reset();
        *LAST.lock().unwrap() = Some(super::snapshot());
        CHECKPOINTS.lock().unwrap().clear();
    }

    /// The phases recorded since `start`, in order, each with its counts (see `NAMES`).
    pub fn take() -> Vec<(&'static str, Vec<u64>)> {
        core::mem::take(&mut *CHECKPOINTS.lock().unwrap())
    }
}
#[cfg(feature = "std")]
pub use checkpoints::{checkpoint, start, take};
