//! Keeps proofs on chain so anyone can re-run the verifier contract with a tiny transaction.
//!
//! A proof is 13–72 KB, too long to put in a link. This contract stores each proof once, in
//! numbered slots, and `run(slot)` passes it to the verifier contract it was made for. The
//! verification itself happens in that verifier contract, unchanged; the answer, or the
//! verifier's error, is returned as is. That lets a link to Stellar Lab's simulateTransaction
//! form carry a transaction of about 200 bytes.
#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, vec, Address, Bytes, Env, IntoVal, Symbol, Val};

#[contracttype]
enum Key {
    Admin,
    Slot(u32),
    Chunk(u32, u32),
}

/// Which verifier a stored proof goes to, and how many chunks it was stored in (a ledger entry
/// holds at most 64 KB).
#[contracttype]
#[derive(Clone)]
pub struct Slot {
    pub verifier: Address,
    pub chunks: u32,
}

#[contract]
pub struct Demo;

#[contractimpl]
impl Demo {
    pub fn __constructor(env: Env, admin: Address) {
        env.storage().instance().set(&Key::Admin, &admin);
    }

    /// Stores chunk `index` (of `chunks`) of the proof in `slot`, to be checked by `verifier`.
    /// Only the admin can store.
    pub fn put(env: Env, slot: u32, verifier: Address, chunks: u32, index: u32, data: Bytes) {
        let admin: Address = env.storage().instance().get(&Key::Admin).unwrap();
        admin.require_auth();
        let max = env.storage().max_ttl();
        let store = env.storage().persistent();
        store.set(&Key::Slot(slot), &Slot { verifier, chunks });
        store.set(&Key::Chunk(slot, index), &data);
        store.extend_ttl(&Key::Slot(slot), max, max);
        store.extend_ttl(&Key::Chunk(slot, index), max, max);
        env.storage().instance().extend_ttl(max, max);
    }

    /// Sends the proof stored in `slot` to its verifier and returns the verifier's answer.
    pub fn run(env: Env, slot: u32) -> Val {
        let store = env.storage().persistent();
        let s: Slot = store.get(&Key::Slot(slot)).unwrap();
        let mut proof = Bytes::new(&env);
        for i in 0..s.chunks {
            let chunk: Bytes = store.get(&Key::Chunk(slot, i)).unwrap();
            proof.append(&chunk);
        }
        env.invoke_contract(&s.verifier, &Symbol::new(&env, "verify"), vec![&env, proof.into_val(&env)])
    }
}
