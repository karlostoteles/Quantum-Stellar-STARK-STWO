//! Runs the compiled recovery prototype inside the Soroban host, with the network's metering.
//! Fake seeds only. Build first with scripts/build-recovery.sh, which writes
//! target/wasm32v1-none/contract/recovery_default.wasm and recovery_wasm_ec.wasm.
//! The key check is exercised through the wasm-ec build, which is where check_key and
//! check_key_wasm live (they are compiled out of the deployed default build).
use curve25519_dalek::constants::EIGHT_TORSION;
use curve25519_dalek::edwards::{CompressedEdwardsY, EdwardsPoint};
use ed25519_dalek::SigningKey;
use soroban_sdk::{testutils::Ledger, Bytes, BytesN, Env};

mod recovery {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/contract/recovery_default.wasm");
}
mod recovery_wasm_ec {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/contract/recovery_wasm_ec.wasm");
}
use recovery::Error;

const ML_DSA_44_PUBLIC_KEY_BYTES: usize = 1312;
const DELAY: u32 = 100;

struct Owner {
    a: BytesN<32>,
    k: BytesN<32>,
}

fn env_at(ledger: u32) -> Env {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    env.ledger().set_sequence_number(ledger);
    env
}

fn at(env: &Env, ledger: u32) {
    env.ledger().set_sequence_number(ledger);
}

fn owner(env: &Env, seed: u8) -> Owner {
    let k = [seed; 32];
    let a = SigningKey::from_bytes(&k).verifying_key().to_bytes();
    Owner { a: BytesN::from_array(env, &a), k: BytesN::from_array(env, &k) }
}

fn new_key(env: &Env, fill: u8) -> Bytes {
    Bytes::from_slice(env, &[fill; ML_DSA_44_PUBLIC_KEY_BYTES])
}

/// What a wallet computes off chain: C = SHA-256(tag || network || A || k || SHA-256(new key)).
fn commitment(env: &Env, o: &Owner, key: &Bytes) -> BytesN<32> {
    let key_hash: BytesN<32> = env.crypto().sha256(key).into();
    let mut buf = Bytes::from_slice(env, b"stellar-seed-recovery-v0");
    buf.extend_from_array(&env.ledger().network_id().to_array());
    buf.extend_from_array(&o.a.to_array());
    buf.extend_from_array(&o.k.to_array());
    buf.extend_from_array(&key_hash.to_array());
    env.crypto().sha256(&buf).into()
}

/// A recovery contract whose cutoff has passed.
fn deploy(env: &Env) -> recovery::Client<'_> {
    deploy_with_cutoff(env, 0)
}

fn deploy_with_cutoff(env: &Env, cutoff: u32) -> recovery::Client<'_> {
    let id = env.register(recovery::WASM, (cutoff, DELAY));
    recovery::Client::new(env, &id)
}

/// The wasm-ec build, which carries the measurement-only check_key / check_key_wasm.
fn deploy_measure(env: &Env) -> recovery_wasm_ec::Client<'_> {
    let id = env.register(recovery_wasm_ec::WASM, (0u32, DELAY));
    recovery_wasm_ec::Client::new(env, &id)
}

fn instructions(env: &Env) -> i64 {
    env.cost_estimate().resources().instructions
}

fn refused<T, E: core::fmt::Debug>(r: Result<T, Result<Error, E>>) -> Error {
    match r {
        Err(Ok(e)) => e,
        Err(Err(e)) => panic!("not a contract error: {e:?}"),
        Ok(_) => panic!("expected the call to be refused"),
    }
}

#[test]
fn commit_then_reveal_recovers() {
    let env = env_at(1000);
    let c = deploy(&env);
    let o = owner(&env, 7);
    let key = new_key(&env, 0xA5);
    c.commit(&commitment(&env, &o, &key));
    println!("commit: {} instructions", instructions(&env));
    at(&env, 1000 + DELAY);
    let r = c.reveal(&o.a, &o.k, &key);
    println!("reveal with a {ML_DSA_44_PUBLIC_KEY_BYTES}-byte new key: {} instructions", instructions(&env));
    assert!(instructions(&env) < 400_000_000);
    let expected: BytesN<32> = env.crypto().sha256(&key).into();
    assert_eq!(r.new_key_sha256, expected);
    assert_eq!((r.committed_at, r.recovered_at), (1000, 1000 + DELAY));
    assert_eq!(c.recovered(&o.a), Some(r));
}

#[test]
fn a_commitment_younger_than_the_delay_is_refused() {
    let env = env_at(1000);
    let c = deploy(&env);
    let o = owner(&env, 8);
    let key = new_key(&env, 0xA5);
    c.commit(&commitment(&env, &o, &key));
    assert_eq!(refused(c.try_reveal(&o.a, &o.k, &key)), Error::TooEarly);
    at(&env, 1000 + DELAY - 1);
    assert_eq!(refused(c.try_reveal(&o.a, &o.k, &key)), Error::TooEarly);
}

#[test]
fn reveals_wait_for_the_cutoff_and_an_old_commitment_is_final_at_once() {
    let env = env_at(1000);
    let c = deploy_with_cutoff(&env, 5000);
    let o = owner(&env, 9);
    let key = new_key(&env, 0xA5);
    c.commit(&commitment(&env, &o, &key));
    at(&env, 4999);
    assert_eq!(refused(c.try_reveal(&o.a, &o.k, &key)), Error::BeforeCutoff);
    at(&env, 5000);
    assert_eq!(c.reveal(&o.a, &o.k, &key).committed_at, 1000);
}

#[test]
fn a_commitment_made_just_before_the_cutoff_is_not_yet_old_enough() {
    // Committing less than DELAY ledgers before the cutoff does not beat the delay: the age is
    // counted in ledgers, independent of the cutoff.
    let env = env_at(4950);
    let c = deploy_with_cutoff(&env, 5000);
    let o = owner(&env, 10);
    let key = new_key(&env, 0xA5);
    c.commit(&commitment(&env, &o, &key));
    at(&env, 5000);
    assert_eq!(refused(c.try_reveal(&o.a, &o.k, &key)), Error::TooEarly);
    at(&env, 4950 + DELAY);
    assert_eq!(c.reveal(&o.a, &o.k, &key).committed_at, 4950);
}

#[test]
fn a_wrong_seed_or_a_different_key_opens_no_commitment() {
    let env = env_at(1000);
    let c = deploy(&env);
    let o = owner(&env, 11);
    let key = new_key(&env, 0xA5);
    c.commit(&commitment(&env, &o, &key));
    at(&env, 1000 + DELAY);
    let wrong_k = BytesN::from_array(&env, &[12u8; 32]);
    assert_eq!(refused(c.try_reveal(&o.a, &wrong_k, &key)), Error::UnknownCommitment);
    assert_eq!(refused(c.try_reveal(&o.a, &o.k, &new_key(&env, 0x5A))), Error::UnknownCommitment);
}

#[test]
fn copying_a_pending_commitment_changes_nothing() {
    // An attacker sees C before it lands and submits it again. C binds the owner's key, so the copy
    // is the owner's own commitment, and its first ledger is kept.
    let env = env_at(1000);
    let c = deploy(&env);
    let o = owner(&env, 13);
    let key = new_key(&env, 0xA5);
    let cm = commitment(&env, &o, &key);
    c.commit(&cm);
    at(&env, 1050);
    c.commit(&cm);
    at(&env, 1000 + DELAY);
    assert_eq!(c.reveal(&o.a, &o.k, &key).committed_at, 1000);
}

#[test]
fn the_first_valid_reveal_is_final() {
    let env = env_at(1000);
    let c = deploy(&env);
    let o = owner(&env, 14);
    let owner_key = new_key(&env, 0xA5);
    let thief_key = new_key(&env, 0x5A);
    c.commit(&commitment(&env, &o, &owner_key));
    c.commit(&commitment(&env, &o, &thief_key));
    at(&env, 1000 + DELAY);
    c.reveal(&o.a, &o.k, &owner_key);
    // A competing reveal with its own aged commitment is refused once A is recovered.
    assert_eq!(refused(c.try_reveal(&o.a, &o.k, &thief_key)), Error::AlreadyRecovered);
    let owner_hash: BytesN<32> = env.crypto().sha256(&owner_key).into();
    assert_eq!(c.recovered(&o.a).unwrap().new_key_sha256, owner_hash);
}

#[test]
fn a_thief_who_reads_the_seed_from_a_pending_reveal_is_too_late() {
    let env = env_at(1000);
    let c = deploy(&env);
    let o = owner(&env, 15);
    let owner_key = new_key(&env, 0xA5);
    let thief_key = new_key(&env, 0x5A);
    c.commit(&commitment(&env, &o, &owner_key));
    // The owner's reveal is broadcast at 1100 and held up; the thief reads k from it and commits.
    at(&env, 1100);
    c.commit(&commitment(&env, &o, &thief_key));
    at(&env, 1101);
    assert_eq!(refused(c.try_reveal(&o.a, &o.k, &thief_key)), Error::TooEarly);
    // The owner's reveal, or a retry, lands within the delay.
    at(&env, 1150);
    c.reveal(&o.a, &o.k, &owner_key);
    at(&env, 1100 + DELAY);
    assert_eq!(refused(c.try_reveal(&o.a, &o.k, &thief_key)), Error::AlreadyRecovered);
}

#[test]
fn an_owner_kept_out_for_the_whole_delay_loses() {
    // The assumption the scheme rests on: the owner must get one transaction in within the delay
    // of k first leaking. If a thief learns k and the owner never lands a reveal, the thief wins.
    let env = env_at(1000);
    let c = deploy(&env);
    let o = owner(&env, 16);
    let owner_key = new_key(&env, 0xA5);
    let thief_key = new_key(&env, 0x5A);
    c.commit(&commitment(&env, &o, &owner_key));
    at(&env, 1100);
    c.commit(&commitment(&env, &o, &thief_key));
    at(&env, 1100 + DELAY);
    c.reveal(&o.a, &o.k, &thief_key);
    assert_eq!(refused(c.try_reveal(&o.a, &o.k, &owner_key)), Error::AlreadyRecovered);
}

#[test]
fn the_key_check_rejects_a_mixed_order_point() {
    // verify_strict rejects a small-order A but not A + T for an 8-torsion T. The odd-h grind makes
    // the host's cofactor-free check reject it anyway, so the contract matches "exactly [s]B = A".
    let env = env_at(1000);
    let m = deploy_measure(&env);
    let o = owner(&env, 17);
    m.check_key(&o.a, &o.k, &BytesN::from_array(&env, &[0u8; 32])); // A = [s]B: accepted
    let a_real = CompressedEdwardsY(o.a.to_array()).decompress().unwrap();
    for t in [1usize, 2, 3] {
        // EIGHT_TORSION[t] has order 8, 4 or 8; add it to A and expect a trap.
        let tainted: EdwardsPoint = a_real + EIGHT_TORSION[t];
        let a_bad = BytesN::from_array(&env, &tainted.compress().to_bytes());
        assert!(
            m.try_check_key(&a_bad, &o.k, &BytesN::from_array(&env, &[0u8; 32])).is_err(),
            "A + EIGHT_TORSION[{t}] must be rejected"
        );
    }
}

#[test]
fn a_seed_that_does_not_make_the_key_fails() {
    let env = env_at(1000);
    let m = deploy_measure(&env);
    let o = owner(&env, 18);
    let zero = BytesN::from_array(&env, &[0u8; 32]);
    assert!(m.try_check_key(&o.a, &o.k, &zero).is_ok());
    let wrong_k = BytesN::from_array(&env, &[19u8; 32]);
    assert!(m.try_check_key(&o.a, &wrong_k, &zero).is_err());
    // A reveal whose commitment opens with the seed of another account fails the key check (a trap,
    // not a contract error) and records nothing.
    let c = deploy(&env);
    let other = owner(&env, 20);
    let key = new_key(&env, 0xA5);
    let liar = Owner { a: o.a.clone(), k: other.k.clone() };
    c.commit(&commitment(&env, &liar, &key));
    at(&env, 1000 + DELAY);
    assert!(matches!(c.try_reveal(&liar.a, &liar.k, &key), Err(Err(_))));
    assert_eq!(c.recovered(&o.a), None);
}

#[test]
fn the_network_id_is_bound() {
    // The same (A, k, new key) commits to different C under different network ids.
    let env = env_at(1000);
    let o = owner(&env, 21);
    let key = new_key(&env, 0xA5);
    env.ledger().set_network_id([1u8; 32]);
    let c1 = commitment(&env, &o, &key);
    env.ledger().set_network_id([2u8; 32]);
    let c2 = commitment(&env, &o, &key);
    assert_ne!(c1.to_array(), c2.to_array());
}

#[test]
fn cost_of_the_key_check_host_versus_wasm() {
    let env = env_at(1000);
    let m = deploy_measure(&env);
    let o = owner(&env, 22);
    let zero = BytesN::from_array(&env, &[0u8; 32]);
    m.check_key(&o.a, &o.k, &zero);
    let host = instructions(&env);
    assert!(m.check_key_wasm(&o.a, &o.k));
    let wasm = instructions(&env);
    println!("key check via host ed25519_verify: {host} instructions; via Curve25519 in Wasm: {wasm}");
}
