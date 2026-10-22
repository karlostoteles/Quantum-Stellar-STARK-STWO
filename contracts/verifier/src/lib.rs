//! Verifies Stwo proofs (StarkWare's Circle STARK prover) inside a Soroban transaction.
//! The check is hash-based (Blake2s) and does not rely on elliptic curves.
#![no_std]

use soroban_sdk::{contract, contracterror, contractevent, contractimpl, contracttype, Bytes, BytesN, Env};
use stwo_soroban_air::{verify_envelope, Envelope, VerifyError};

#[cfg(target_family = "wasm")]
mod no_rng {
    fn unsupported(_: &mut [u8]) -> Result<(), getrandom::Error> {
        Err(getrandom::Error::UNSUPPORTED)
    }
    getrandom::register_custom_getrandom!(unsupported);
}

/// With the `keccak` feature, Stwo's Keccak-256 hasher calls this to hash through the host.
#[cfg(feature = "keccak")]
mod host_keccak {
    use core::cell::RefCell;
    use soroban_sdk::{Bytes, Env};

    struct EnvCell(RefCell<Option<Env>>);
    // Contracts run single-threaded; the cell only carries the Env of the current call.
    unsafe impl Sync for EnvCell {}
    static ENV: EnvCell = EnvCell(RefCell::new(None));

    pub fn set(env: &Env) {
        *ENV.0.borrow_mut() = Some(env.clone());
    }

    #[no_mangle]
    pub extern "Rust" fn stwo_host_keccak256(data: *const u8, len: usize, out: *mut u8) {
        let cell = ENV.0.borrow();
        let env = cell.as_ref().unwrap();
        let input = unsafe { core::slice::from_raw_parts(data, len) };
        let digest = env.crypto().keccak256(&Bytes::from_slice(env, input)).to_array();
        unsafe { core::ptr::copy_nonoverlapping(digest.as_ptr(), out, 32) };
    }
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// The bytes are not a proof envelope.
    MalformedProof = 1,
    /// Statement values out of range.
    UnsupportedStatement = 2,
    /// Proof parameters below the accepted security level.
    WeakSecurity = 3,
    /// The proof does not verify.
    InvalidProof = 4,
}

/// What a successful call proved.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Verified {
    pub log_n_rows: u32,
    pub a0: u32,
    pub a1: u32,
    pub last: u32,
    pub security_bits: u32,
    pub proof_sha256: BytesN<32>,
}

#[contractevent(topics = ["stark_verified"])]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StarkVerified {
    pub a0: u32,
    pub a1: u32,
    pub last: u32,
    pub security_bits: u32,
    pub proof_sha256: BytesN<32>,
}

#[contract]
pub struct StwoVerifier;

#[contractimpl]
impl StwoVerifier {
    /// Verifies a postcard-encoded proof envelope produced by `stwo-soroban-prover`.
    pub fn verify(env: Env, proof: Bytes) -> Result<Verified, Error> {
        #[cfg(feature = "keccak")]
        host_keccak::set(&env);
        let proof_sha256: BytesN<32> = env.crypto().sha256(&proof).into();
        let raw = proof.to_alloc_vec();
        let envelope: Envelope = postcard::from_bytes(&raw).map_err(|_| Error::MalformedProof)?;
        drop(raw);
        let s = envelope.statement;
        let security_bits = verify_envelope(envelope).map_err(|e| match e {
            VerifyError::UnsupportedStatement => Error::UnsupportedStatement,
            VerifyError::WeakSecurity => Error::WeakSecurity,
            VerifyError::InvalidProof => Error::InvalidProof,
        })?;
        StarkVerified { a0: s.a0, a1: s.a1, last: s.last, security_bits, proof_sha256: proof_sha256.clone() }
            .publish(&env);
        Ok(Verified { log_n_rows: s.log_n_rows, a0: s.a0, a1: s.a1, last: s.last, security_bits, proof_sha256 })
    }

    /// Profiling only: decodes the envelope and stops.
    #[cfg(feature = "probe")]
    pub fn decode_only(_env: Env, proof: Bytes) -> u32 {
        let raw = proof.to_alloc_vec();
        let envelope: Envelope = postcard::from_bytes(&raw).unwrap();
        envelope.proof.commitments.len() as u32
    }

    /// Profiling only: decodes the envelope and runs the verifier up to phase `until`
    /// (see `stwo_soroban_air::probe::PHASES`; 0 = decode only). Returns the security bits.
    #[cfg(feature = "probe")]
    pub fn verify_phase(_env: Env, proof: Bytes, until: u32) -> u32 {
        let raw = proof.to_alloc_vec();
        let envelope: Envelope = postcard::from_bytes(&raw).unwrap();
        drop(raw);
        stwo_soroban_air::probe::verify_until(envelope, until as usize).unwrap()
    }

    /// Profiling only: `n` iterations of one operation on values the compiler cannot fold, so the
    /// cost per operation is (cost(2n) - cost(n)) / n. `op` selects the operation:
    /// 0 loop only · 1 M31 add · 2 M31 mul · 3 M31 inverse · 4 CM31 mul · 5 QM31 add · 6 QM31 mul ·
    /// 7 QM31 mul by M31 · 8 QM31 inverse · 9 Blake2s of 64 bytes (one compression) ·
    /// 10 circle point by scalar (a FRI twiddle) · 11 bn254_fr_mul through the host (precedent) ·
    /// 12 bn254_fr_add through the host · 13 sha256 of 64 bytes through the host.
    #[cfg(feature = "probe")]
    pub fn bench(env: Env, op: u32, n: u32) -> u32 {
        use stwo::core::circle::{CirclePoint, CirclePointIndex};
        use stwo::core::fields::cm31::CM31;
        use stwo::core::fields::m31::M31;
        use stwo::core::fields::qm31::QM31;
        use stwo::core::fields::FieldExpOps;
        use stwo::core::vcs_lifted::merkle_hasher::MerkleHasherLifted;
        use stwo_soroban_air::Hasher;
        let seed = M31::from(n.wrapping_mul(2654435761) % 2147483647);
        let mut acc = 0u32;
        match op {
            0 => {
                let mut x = n;
                for i in 0..n {
                    x = x.wrapping_mul(3).wrapping_add(i);
                }
                acc = x;
            }
            1 => {
                let (mut x, y) = (seed, M31::from(7u32));
                for _ in 0..n {
                    x = x + y;
                }
                acc = x.0;
            }
            2 => {
                let (mut x, y) = (seed, M31::from(7u32));
                for _ in 0..n {
                    x = x * y;
                }
                acc = x.0;
            }
            3 => {
                let mut x = seed + M31::from(1u32);
                for _ in 0..n {
                    x = x.inverse() + M31::from(1u32);
                }
                acc = x.0;
            }
            4 => {
                let (mut x, y) = (CM31::from_m31(seed, M31::from(3u32)), CM31::from_m31(M31::from(5u32), M31::from(7u32)));
                for _ in 0..n {
                    x = x * y;
                }
                acc = x.0 .0;
            }
            5 => {
                let (mut x, y) = (QM31::from_m31(seed, seed, seed, seed), QM31::from_u32_unchecked(1, 2, 3, 4));
                for _ in 0..n {
                    x = x + y;
                }
                acc = x.0 .0 .0;
            }
            6 => {
                let (mut x, y) = (QM31::from_m31(seed, seed, seed, seed), QM31::from_u32_unchecked(1, 2, 3, 4));
                for _ in 0..n {
                    x = x * y;
                }
                acc = x.0 .0 .0;
            }
            7 => {
                let (mut x, y) = (QM31::from_m31(seed, seed, seed, seed), M31::from(7u32));
                for _ in 0..n {
                    x = x * y;
                }
                acc = x.0 .0 .0;
            }
            8 => {
                let mut x = QM31::from_m31(seed, seed, seed, seed) + QM31::from_u32_unchecked(1, 0, 0, 0);
                for _ in 0..n {
                    x = x.inverse() + QM31::from_u32_unchecked(1, 0, 0, 0);
                }
                acc = x.0 .0 .0;
            }
            9 => {
                let mut h = stwo::core::vcs::blake2_hash::Blake2sHash([n as u8; 32]);
                for _ in 0..n {
                    h = Hasher::hash_children((h, h));
                }
                acc = h.0[0] as u32;
            }
            10 => {
                let mut p = CirclePoint::<M31>::zero();
                for i in 0..n {
                    p = p + CirclePointIndex(seed.0.wrapping_add(i) as usize).to_point();
                }
                acc = p.x.0;
            }
            11 | 12 => {
                let bn = env.crypto().bn254();
                let mut x = soroban_sdk::crypto::bn254::Bn254Fr::from_u256(soroban_sdk::U256::from_u32(&env, seed.0));
                let y = soroban_sdk::crypto::bn254::Bn254Fr::from_u256(soroban_sdk::U256::from_u32(&env, 7));
                for _ in 0..n {
                    x = if op == 11 { bn.fr_mul(&x, &y) } else { bn.fr_add(&x, &y) };
                }
                acc = x.to_u256().to_be_bytes().get(31).unwrap() as u32;
            }
            13 => {
                let mut b = soroban_sdk::BytesN::<32>::from_array(&env, &[n as u8; 32]);
                for _ in 0..n {
                    let mut input = Bytes::from_array(&env, &b.to_array());
                    input.append(&Bytes::from_array(&env, &b.to_array()));
                    b = env.crypto().sha256(&input).into();
                }
                acc = b.get(0).unwrap() as u32;
            }
            _ => panic!(),
        }
        acc
    }

    /// Sequence length of the statement this contract checks (a_0 .. a_{terms-1}).
    pub fn terms(_env: Env) -> u32 {
        stwo_soroban_air::SEQ_LEN as u32
    }
}
