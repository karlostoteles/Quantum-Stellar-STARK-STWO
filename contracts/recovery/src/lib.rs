//! Prototype: recovering an Ed25519 account after classical signatures stop being accepted.
//!
//! A Stellar secret key (S...) is the 32-byte RFC 8032 seed k. The account key is A = [s]B with
//! s = clamp(SHA-512(k)[0..32]). A quantum attacker who breaks A learns s mod L, which is enough to
//! sign, but not k, which would take a SHA-512 preimage. So once Ed25519 is no longer accepted, k
//! still identifies the owner, and s is no longer a secret.
//!
//! Recovery with no zero-knowledge proof, in the commit, delay, reveal pattern of Fawkescoin
//! (Bonneau and Miller, 2014) and of Stewart et al. for Bitcoin (ePrint 2018/213):
//!   1. commit(C), at any time, before the cutoff too, with
//!      C = SHA-256(tag || network || A || k || SHA-256(new key)).
//!      C reveals nothing and binds the new key, so copying it from a pending transaction gains an
//!      attacker nothing.
//!   2. reveal(A, k, new key), once the cutoff has passed and C is at least `delay` ledgers old.
//!      The contract recomputes C, derives s from k and checks [s]B = A. The first reveal that
//!      passes rekeys the account, and later ones are refused.
//! A reveal makes k public. Whoever reads it from a pending transaction can commit to it with their
//! own key, but must then wait `delay` ledgers, while the owner's reveal, or a retry, only has to
//! land before that. So the owner must be able to get one transaction in within `delay` ledgers.
//! A commitment made long before the cutoff is already old enough, and its reveal is final at once.
//!
//! The check [s]B = A uses the host's ed25519_verify instead of curve arithmetic in Wasm: the
//! contract builds a signature on C with nonce point R = B, that is S = 1 + h*s mod L with
//! h = SHA-512(B || A || C || ctr) mod L. The host accepts it exactly when [S]B - [h]A = B, which
//! is [h]([s]B - A) = 0. The host's verify_strict rejects a small-order A but not A plus an
//! 8-torsion point, so the contract grinds the one-byte ctr until h is odd: an odd h kills no
//! nonzero 2-, 4- or 8-torsion component, so [h]([s]B - A) = 0 forces [s]B = A exactly, for any A
//! the host accepts. This is the Ed25519 counterpart of using Ethereum's ecrecover for a scalar
//! multiplication. It needs only s, which is fine after the cutoff because s is public by then.
//! A host verifier that multiplied by the cofactor would break this check.
//!
//! Testnet prototype with fake seeds only. A real recovery of classic G... accounts would be a
//! protocol rule, because a contract cannot change a classic account's signers.
#![no_std]
use curve25519_dalek::scalar::Scalar;
use sha2::{Digest, Sha512};
use soroban_sdk::{contract, contracterror, contractevent, contractimpl, contracttype, Bytes, BytesN, Env};

/// Compressed Edwards Y coordinate of the Ed25519 base point B.
const BASEPOINT: [u8; 32] = [
    0x58, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
    0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
];
/// Domain separation for the commitment hash.
pub const TAG: &[u8; 24] = b"stellar-seed-recovery-v0";

#[contracttype]
enum Key {
    /// (cutoff ledger, delay in ledgers).
    Params,
    /// Commitment C -> ledger it was first seen.
    Commit(BytesN<32>),
    /// Account key A -> its recovery.
    Recovered(BytesN<32>),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Recovery {
    /// SHA-256 of the new key.
    pub new_key_sha256: BytesN<32>,
    /// Ledger of the commitment this recovery opened.
    pub committed_at: u32,
    /// Ledger of the reveal.
    pub recovered_at: u32,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    /// No commitment matches (network, A, k, new key).
    UnknownCommitment = 1,
    /// The commitment is younger than the delay.
    TooEarly = 2,
    /// This account was already recovered.
    AlreadyRecovered = 3,
    /// Ed25519 is still accepted: a reveal now would hand k to anyone who can still sign with it.
    BeforeCutoff = 4,
}

#[contractevent(topics = ["recovered"])]
pub struct Recovered {
    #[topic]
    pub account: BytesN<32>,
    pub new_key_sha256: BytesN<32>,
    pub committed_at: u32,
}

#[contract]
pub struct SeedRecovery;

#[contractimpl]
impl SeedRecovery {
    /// `cutoff`: first ledger in which reveals are accepted. `delay`: minimum age of a commitment, in ledgers.
    pub fn __constructor(env: Env, cutoff: u32, delay: u32) {
        assert!(delay > 0);
        env.storage().instance().set(&Key::Params, &(cutoff, delay));
    }

    /// (cutoff ledger, delay in ledgers).
    pub fn params(env: Env) -> (u32, u32) {
        env.storage().instance().get(&Key::Params).unwrap()
    }

    /// Step 1. Records the ledger in which C was first committed; committing it again changes nothing.
    pub fn commit(env: Env, c: BytesN<32>) {
        let key = Key::Commit(c);
        let store = env.storage().persistent();
        if !store.has(&key) {
            store.set(&key, &env.ledger().sequence());
        }
    }

    /// Step 2. Opens a commitment old enough and records the new key for A, if A has none yet.
    pub fn reveal(env: Env, a: BytesN<32>, k: BytesN<32>, new_key: Bytes) -> Result<Recovery, Error> {
        let now = env.ledger().sequence();
        let (cutoff, delay) = Self::params(env.clone());
        if now < cutoff {
            return Err(Error::BeforeCutoff);
        }
        let store = env.storage().persistent();
        let done = Key::Recovered(a.clone());
        if store.has(&done) {
            return Err(Error::AlreadyRecovered);
        }
        let new_key_sha256: BytesN<32> = env.crypto().sha256(&new_key).into();
        let c = commitment(&env, &a, &k, &new_key_sha256);
        let committed_at: u32 = store.get(&Key::Commit(c.clone())).ok_or(Error::UnknownCommitment)?;
        if now < committed_at.saturating_add(delay) {
            return Err(Error::TooEarly);
        }
        check_key_host(&env, &a, &k, &c);
        let recovery = Recovery { new_key_sha256, committed_at, recovered_at: now };
        store.set(&done, &recovery);
        Recovered { account: a, new_key_sha256: recovery.new_key_sha256.clone(), committed_at }.publish(&env);
        Ok(recovery)
    }

    /// The recovery recorded for an account, if any.
    pub fn recovered(env: Env, a: BytesN<32>) -> Option<Recovery> {
        env.storage().persistent().get(&Key::Recovered(a))
    }

    /// Measurement only, excluded from the deployed build: the same key check as reveal, with
    /// nothing stored. Traps on a wrong key.
    #[cfg(feature = "wasm-ec")]
    pub fn check_key(env: Env, a: BytesN<32>, k: BytesN<32>, m: BytesN<32>) {
        check_key_host(&env, &a, &k, &m);
    }

    /// Measurement only: [s]B computed with Curve25519 arithmetic compiled into the contract.
    #[cfg(feature = "wasm-ec")]
    pub fn check_key_wasm(_env: Env, a: BytesN<32>, k: BytesN<32>) -> bool {
        let s = Scalar::from_bytes_mod_order(clamped_scalar_bytes(&k.to_array()));
        curve25519_dalek::edwards::EdwardsPoint::mul_base(&s).compress().to_bytes() == a.to_array()
    }
}

/// C = SHA-256(tag || network id || A || k || SHA-256(new key)).
fn commitment(env: &Env, a: &BytesN<32>, k: &BytesN<32>, new_key_sha256: &BytesN<32>) -> BytesN<32> {
    let mut buf = Bytes::from_array(env, TAG);
    buf.extend_from_array(&env.ledger().network_id().to_array());
    buf.extend_from_array(&a.to_array());
    buf.extend_from_array(&k.to_array());
    buf.extend_from_array(&new_key_sha256.to_array());
    env.crypto().sha256(&buf).into()
}

/// s = clamp(SHA-512(k)[0..32]) as in RFC 8032 section 5.1.5.
fn clamped_scalar_bytes(k: &[u8; 32]) -> [u8; 32] {
    let h = Sha512::digest(k);
    let mut s = [0u8; 32];
    s.copy_from_slice(&h[..32]);
    s[0] &= 248;
    s[31] &= 127;
    s[31] |= 64;
    s
}

/// Traps unless [s]B = A, with s derived from k. See the module comment for why this works.
/// The message signed is `m || ctr`, with ctr the one byte that makes h odd.
fn check_key_host(env: &Env, a: &BytesN<32>, k: &BytesN<32>, m: &BytesN<32>) {
    let a_bytes = a.to_array();
    let s = Scalar::from_bytes_mod_order(clamped_scalar_bytes(&k.to_array()));
    let mut msg = [0u8; 33];
    msg[..32].copy_from_slice(&m.to_array());
    let mut h = Scalar::ZERO;
    let mut found = false;
    for ctr in 0..=u8::MAX {
        msg[32] = ctr;
        let mut hasher = Sha512::new();
        hasher.update(BASEPOINT);
        hasher.update(a_bytes);
        hasher.update(msg);
        let mut wide = [0u8; 64];
        wide.copy_from_slice(&hasher.finalize());
        h = Scalar::from_bytes_mod_order_wide(&wide);
        // An odd h (as an integer in [0, L)) cannot kill a nonzero 2/4/8-torsion component, so the
        // host's cofactor-free check then holds exactly when [s]B = A. Odd also excludes h = 0.
        if h.as_bytes()[0] & 1 == 1 {
            found = true;
            break;
        }
    }
    assert!(found);
    let big_s = Scalar::ONE + h * s;
    let mut sig = [0u8; 64];
    sig[..32].copy_from_slice(&BASEPOINT);
    sig[32..].copy_from_slice(big_s.as_bytes());
    env.crypto().ed25519_verify(a, &Bytes::from_array(env, &msg), &BytesN::from_array(env, &sig));
}
