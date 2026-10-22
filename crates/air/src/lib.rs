//! What gets proven, and how a proof of it is checked.
//!
//! Statement: the FibonacciSq sequence (a_{n+2} = a_n^2 + a_{n+1}^2 over the Mersenne-31 field,
//! the recurrence of StarkWare's "STARK 101") that starts at `(a0, a1)` has `last` as its term
//! number `SEQ_LEN - 1`. Each trace row holds the whole sequence; the boundary constraints pin
//! every row to the public `a0`, `a1` and `last`, so a valid trace exists only if the claim holds.
//!
//! The same `verify_envelope` runs natively in the prover (as a self-check) and inside the
//! Soroban contract (compiled to wasm32v1-none).
#![no_std]
extern crate alloc;

use num_traits::Zero;
use serde::{Deserialize, Serialize};
use stwo::core::air::Component;
use stwo::core::channel::Channel;
use stwo::core::fields::m31::{BaseField, P};
use stwo::core::fields::qm31::SecureField;
use stwo::core::fields::FieldExpOps;
use stwo::core::fri::FriConfig;
use stwo::core::pcs::{CommitmentSchemeVerifier, PcsConfig};
use stwo::core::proof::StarkProof;
use stwo::core::verifier::verify;
use stwo_constraint_framework::{EvalAtRow, FrameworkComponent, FrameworkEval, TraceLocationAllocator};

/// Stwo's default Blake2s channel, the same hash StarkWare's examples use.
#[cfg(not(feature = "keccak"))]
pub type MerkleChannel = stwo::core::vcs_lifted::blake2_merkle::Blake2sM31MerkleChannel;
#[cfg(not(feature = "keccak"))]
pub type ProofChannel = stwo::core::channel::Blake2sM31Channel;
#[cfg(not(feature = "keccak"))]
pub type Hasher = stwo::core::vcs_lifted::blake2_merkle::Blake2sMerkleHasher;

/// Stwo's Keccak-256 channel; on Soroban the hashing runs in the host (`keccak256`).
#[cfg(feature = "keccak")]
pub type MerkleChannel = stwo::core::vcs_lifted::keccak256_merkle::Keccak256MerkleChannel;
#[cfg(feature = "keccak")]
pub type ProofChannel = stwo::core::channel::Keccak256Channel;
#[cfg(feature = "keccak")]
pub type Hasher = stwo::core::vcs_lifted::keccak256_merkle::Keccak256MerkleHasher;

/// Terms per sequence: a_0 .. a_{SEQ_LEN-1}.
pub const SEQ_LEN: usize = 100;
/// Accepted trace heights (rows = 2^log_n_rows).
pub const MIN_LOG_ROWS: u32 = 4;
pub const MAX_LOG_ROWS: u32 = 16;
/// Minimum security accepted on chain: pow_bits + log_blowup * n_queries (Stwo's own formula,
/// conjectured soundness).
pub const MIN_SECURITY_BITS: u32 = 96;
/// Domain separation for the Fiat-Shamir transcript: "STSB" (Stwo on Soroban), version 1.
pub const TRANSCRIPT_TAG: u32 = 0x5354_5342;
pub const TRANSCRIPT_VERSION: u32 = 1;

/// The public claim. All values are M31 field elements (< 2^31 - 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Statement {
    pub log_n_rows: u32,
    pub a0: u32,
    pub a1: u32,
    pub last: u32,
}

/// What travels on chain: the claim, the FRI parameters the prover used, and the proof.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Envelope {
    pub statement: Statement,
    pub fri_config: FriConfig,
    pub proof: StarkProof<Hasher>,
}

/// The AIR. Degree-2 transition constraints along each row, degree-1 boundary constraints.
#[derive(Clone)]
pub struct FibSqEval {
    pub log_n_rows: u32,
    pub a0: BaseField,
    pub a1: BaseField,
    pub last: BaseField,
}

impl FrameworkEval for FibSqEval {
    fn log_size(&self) -> u32 {
        self.log_n_rows
    }

    fn max_constraint_log_degree_bound(&self) -> u32 {
        self.log_n_rows + 1
    }

    fn evaluate<E: EvalAtRow>(&self, mut eval: E) -> E {
        let mut a = eval.next_trace_mask();
        let mut b = eval.next_trace_mask();
        eval.add_constraint(a.clone() - E::F::from(self.a0));
        eval.add_constraint(b.clone() - E::F::from(self.a1));
        for _ in 2..SEQ_LEN {
            let c = eval.next_trace_mask();
            eval.add_constraint(c.clone() - (a.square() + b.square()));
            a = b;
            b = c;
        }
        eval.add_constraint(b - E::F::from(self.last));
        eval
    }
}

pub type FibSqComponent = FrameworkComponent<FibSqEval>;

#[cfg(feature = "probe")]
pub mod probe;

/// The same component as [`FibSqComponent`], written out by hand for the verifier (feature `fast`):
/// no constraint-framework "info" pass at run time, mask points without a point addition per
/// column, and the constraints evaluated directly. Same trace layout, same constraint order, same
/// values, so it accepts exactly the proofs the framework component accepts. The prover keeps using
/// [`FibSqComponent`].
#[cfg(feature = "fast")]
pub struct FibSqFastComponent(pub FibSqEval);

#[cfg(feature = "fast")]
impl Component for FibSqFastComponent {
    fn n_constraints(&self) -> usize {
        SEQ_LEN + 1
    }

    fn max_constraint_log_degree_bound(&self) -> u32 {
        self.0.log_n_rows + 1
    }

    fn trace_log_degree_bounds(&self) -> stwo::core::pcs::TreeVec<stwo::core::ColumnVec<u32>> {
        stwo::core::pcs::TreeVec::new(alloc::vec![alloc::vec![], alloc::vec![self.0.log_n_rows; SEQ_LEN]])
    }

    fn mask_points(
        &self,
        point: stwo::core::circle::CirclePoint<SecureField>,
        _max_log_degree_bound: u32,
    ) -> stwo::core::pcs::TreeVec<stwo::core::ColumnVec<alloc::vec::Vec<stwo::core::circle::CirclePoint<SecureField>>>> {
        // Every column is read at offset 0: the framework adds the zero point to `point` here.
        stwo::core::pcs::TreeVec::new(alloc::vec![alloc::vec![], alloc::vec![alloc::vec![point]; SEQ_LEN]])
    }

    fn preprocessed_column_indices(&self) -> stwo::core::ColumnVec<usize> {
        alloc::vec![]
    }

    fn evaluate_constraint_quotients_at_point(
        &self,
        point: stwo::core::circle::CirclePoint<SecureField>,
        mask: &stwo::core::pcs::TreeVec<stwo::core::ColumnVec<alloc::vec::Vec<SecureField>>>,
        evaluation_accumulator: &mut stwo::core::air::accumulation::PointEvaluationAccumulator,
        max_log_degree_bound: u32,
    ) {
        use stwo::core::constraints::coset_vanishing;
        use stwo::core::poly::circle::CanonicCoset;
        let denom_inverse = coset_vanishing(CanonicCoset::new(max_log_degree_bound).coset, point).inverse();
        let m = &mask[1];
        // Same order as FibSqEval::evaluate: a - a0, b - a1, the transitions, then last.
        let mut acc = |c: SecureField| evaluation_accumulator.accumulate(denom_inverse * c);
        acc(m[0][0] - SecureField::from(self.0.a0));
        acc(m[1][0] - SecureField::from(self.0.a1));
        for i in 2..SEQ_LEN {
            acc(m[i][0] - (m[i - 2][0].square() + m[i - 1][0].square()));
        }
        acc(m[SEQ_LEN - 1][0] - SecureField::from(self.0.last));
    }
}

#[cfg(feature = "fast")]
pub fn fast_component(s: &Statement) -> FibSqFastComponent {
    FibSqFastComponent(FibSqEval {
        log_n_rows: s.log_n_rows,
        a0: BaseField::from(s.a0),
        a1: BaseField::from(s.a1),
        last: BaseField::from(s.last),
    })
}

/// Computes term `SEQ_LEN - 1` of the sequence starting at (a0, a1).
pub fn fibsq_last(a0: u32, a1: u32) -> u32 {
    let (mut a, mut b) = (BaseField::from(a0), BaseField::from(a1));
    for _ in 2..SEQ_LEN {
        let c = a.square() + b.square();
        a = b;
        b = c;
    }
    b.0
}

/// The commitment-scheme config for a statement: every tree but the (empty) preprocessed one is
/// lifted to the trace's extended domain. Derived from the statement, never read off the proof.
pub fn pcs_config(fri_config: FriConfig, log_n_rows: u32) -> PcsConfig {
    PcsConfig {
        fri_config,
        trace_lifting_log_size: log_n_rows + fri_config.log_blowup_factor,
        preprocessed_lifting_log_size: 0,
    }
}

/// Binds the public claim into the transcript before any commitment.
pub fn mix_statement(channel: &mut ProofChannel, s: &Statement) {
    channel.mix_u32s(&[TRANSCRIPT_TAG, TRANSCRIPT_VERSION, s.log_n_rows, s.a0, s.a1, s.last]);
}

pub fn component(s: &Statement) -> FibSqComponent {
    FrameworkComponent::new(
        &mut TraceLocationAllocator::default(),
        FibSqEval {
            log_n_rows: s.log_n_rows,
            a0: BaseField::from(s.a0),
            a1: BaseField::from(s.a1),
            last: BaseField::from(s.last),
        },
        SecureField::zero(),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyError {
    /// Values out of the field or a trace height outside the accepted range.
    UnsupportedStatement,
    /// FRI parameters outside the accepted ranges or below MIN_SECURITY_BITS.
    WeakSecurity,
    /// The proof's structure or a cryptographic check failed.
    InvalidProof,
}

/// FRI parameters accepted on chain. Deserialization skips FriConfig::new's asserts, so the
/// ranges are checked here.
pub fn check_fri_config(fri: &FriConfig) -> Result<u32, VerifyError> {
    let ok = (1..=8).contains(&fri.log_blowup_factor)
        && fri.log_last_layer_degree_bound <= 10
        && (1..=4).contains(&fri.fold_step)
        && (1..=256).contains(&fri.n_queries)
        && fri.pow_bits <= 32;
    if !ok {
        return Err(VerifyError::WeakSecurity);
    }
    let bits = fri.security_bits();
    if bits < MIN_SECURITY_BITS {
        return Err(VerifyError::WeakSecurity);
    }
    Ok(bits)
}

/// Checks a proof of `envelope.statement`. Returns the security level (bits) it was proven at.
pub fn verify_envelope(envelope: Envelope) -> Result<u32, VerifyError> {
    let Envelope { statement: s, fri_config, proof } = envelope;
    if !(MIN_LOG_ROWS..=MAX_LOG_ROWS).contains(&s.log_n_rows) || s.a0 >= P || s.a1 >= P || s.last >= P {
        return Err(VerifyError::UnsupportedStatement);
    }
    let bits = check_fri_config(&fri_config)?;
    let config = pcs_config(fri_config, s.log_n_rows);
    if proof.config != config || proof.commitments.len() != 3 {
        return Err(VerifyError::InvalidProof);
    }

    #[cfg(not(feature = "fast"))]
    let component = component(&s);
    #[cfg(feature = "fast")]
    let component = fast_component(&s);
    let channel = &mut ProofChannel::default();
    mix_statement(channel, &s);
    let commitment_scheme = &mut CommitmentSchemeVerifier::<MerkleChannel>::new(config);
    let sizes = component.trace_log_degree_bounds();
    commitment_scheme.commit(proof.commitments[0], &sizes[0], channel);
    commitment_scheme.commit(proof.commitments[1], &sizes[1], channel);
    verify::<MerkleChannel>(&[&component], channel, commitment_scheme, proof)
        .map_err(|_| VerifyError::InvalidProof)?;
    Ok(bits)
}
