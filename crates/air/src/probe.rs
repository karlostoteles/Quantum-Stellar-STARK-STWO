//! Profiling only (feature `probe`): the verifier split into phases so the Soroban budget can be
//! read after each one. `verify_until(envelope, k)` runs the same steps as `verify_envelope`, in
//! the same order, and stops after phase `k`. It mirrors Stwo's `verify_ex` and
//! `CommitmentSchemeVerifier::verify_values` (vendor/stwo/src/core/verifier.rs and
//! pcs/verifier.rs); `verify_until(_, PHASES.len())` must give the same answer as `verify_envelope`.
//!
//! The cost of phase k is cost(verify_until(k)) - cost(verify_until(k - 1)), measured by running
//! the contract once per k (contracts/verifier `verify_phase`, profiling/measure-phases.sh).
use core::iter::zip;

use itertools::Itertools;
use stwo::core::air::{Component, Components};
use stwo::core::channel::Channel;
use stwo::core::circle::CirclePoint;
use stwo::core::fields::qm31::{SecureField, SECURE_EXTENSION_DEGREE};
use stwo::core::fri::{CirclePolyDegreeBound, FriVerifier};
use stwo::core::pcs::quotients::{fri_answers, PointSample};
use stwo::core::pcs::utils::prepare_preprocessed_query_positions;
use stwo::core::pcs::{CommitmentSchemeVerifier, TreeVec};
use stwo::core::proof_of_work::OODS_POW_BITS;
use stwo::core::verifier::{COMPOSITION_LOG_SPLIT, PREPROCESSED_TRACE_IDX};

use crate::{check_fri_config, component, mix_statement, pcs_config, Envelope, MerkleChannel, ProofChannel, VerifyError, MAX_LOG_ROWS, MIN_LOG_ROWS};
use stwo::core::fields::m31::P;

/// Phase names, in execution order. Phase 0 is "decode only" (handled by the caller).
pub const PHASES: &[&str] = &[
    "setup_component",      // 1: statement checks, FRI config, FrameworkComponent::new (info pass)
    "commit_trace_trees",   // 2: mix the two trace commitments and the composition commitment, draw coeff
    "oods_pow_and_point",   // 3: OODS proof of work, draw the OODS point, mask points
    "composition_eval",     // 4: evaluate the constraints at the OODS point and compare
    "mix_sampled_values",   // 5: mix all sampled values, draw the FRI random coefficient
    "fri_commit",           // 6: FRI commitment phase (mix layer roots, draw alphas, last layer poly)
    "fri_pow_and_queries",  // 7: FRI proof of work, draw query positions
    "merkle_trace_trees",   // 8: Merkle decommitments of the (preprocessed,) trace and composition trees
    "fri_answers",          // 9: quotients at the queried points
    "fri_decommit",         // 10: FRI layers: Merkle decommitments and folding, last layer check
];

/// Runs the verifier up to and including phase `until` (1-based; 0 = nothing after decoding).
/// Returns Ok(security bits) when every phase run so far passed.
pub fn verify_until(envelope: Envelope, until: usize) -> Result<u32, VerifyError> {
    let Envelope { statement: s, fri_config, proof } = envelope;
    if until == 0 {
        return Ok(0);
    }
    // ---- 1: setup_component
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
    let component = crate::fast_component(&s);
    let channel = &mut ProofChannel::default();
    mix_statement(channel, &s);
    let commitment_scheme = &mut CommitmentSchemeVerifier::<MerkleChannel>::new(config);
    let sizes = component.trace_log_degree_bounds();
    if until == 1 {
        return Ok(bits);
    }
    // ---- 2: commit_trace_trees (verify_ex up to the composition commitment)
    commitment_scheme.commit(proof.commitments[0], &sizes[0], channel);
    commitment_scheme.commit(proof.commitments[1], &sizes[1], channel);
    let components_list: [&dyn Component; 1] = [&component];
    let n_preprocessed_columns = commitment_scheme.trees[PREPROCESSED_TRACE_IDX].column_log_sizes.len();
    let components = Components { components: components_list.to_vec(), n_preprocessed_columns };
    let split_composition_log_degree_bound = components.composition_log_degree_bound() - COMPOSITION_LOG_SPLIT;
    let lifting_log_size = commitment_scheme.config.trace_lifting_log_size;
    let lifting_log_size =
        lifting_log_size.max(split_composition_log_degree_bound + commitment_scheme.config.fri_config.log_blowup_factor);
    let max_log_degree_bound = lifting_log_size - commitment_scheme.config.fri_config.log_blowup_factor;
    let random_coeff = channel.draw_secure_felt();
    commitment_scheme.commit(
        *proof.commitments.last().unwrap(),
        &[max_log_degree_bound; 2 * SECURE_EXTENSION_DEGREE],
        channel,
    );
    if until == 2 {
        return Ok(bits);
    }
    // ---- 3: oods_pow_and_point
    if !channel.verify_pow_nonce(OODS_POW_BITS, proof.oods_proof_of_work) {
        return Err(VerifyError::InvalidProof);
    }
    channel.mix_u64(proof.oods_proof_of_work);
    let oods_point = CirclePoint::<SecureField>::get_random_point(channel);
    let mut sample_points = components.mask_points(oods_point, max_log_degree_bound, false);
    sample_points.push(alloc::vec![alloc::vec![oods_point]; 2 * SECURE_EXTENSION_DEGREE]);
    if until == 3 {
        return Ok(bits);
    }
    // ---- 4: composition_eval (StarkProof::extract_composition_oods_eval is pub(crate); same steps)
    let composition_oods_eval = {
        let last = proof.sampled_values.0.last().ok_or(VerifyError::InvalidProof)?;
        let evals: alloc::vec::Vec<SecureField> = last
            .iter()
            .map(|columns| match &columns[..] {
                [eval] => Ok(*eval),
                _ => Err(VerifyError::InvalidProof),
            })
            .collect::<Result<_, _>>()?;
        let evals: [SecureField; 2 * SECURE_EXTENSION_DEGREE] = evals.try_into().map_err(|_| VerifyError::InvalidProof)?;
        let (left, right) = evals.split_at(SECURE_EXTENSION_DEGREE);
        let left_eval = SecureField::from_partial_evals(left.try_into().unwrap());
        let right_eval = SecureField::from_partial_evals(right.try_into().unwrap());
        left_eval + oods_point.repeated_double(max_log_degree_bound - 1).x * right_eval
    };
    if composition_oods_eval
        != components.eval_composition_polynomial_at_point(oods_point, &proof.sampled_values, random_coeff, max_log_degree_bound)
    {
        return Err(VerifyError::InvalidProof);
    }
    if until == 4 {
        return Ok(bits);
    }
    // ---- 5: mix_sampled_values (verify_values starts here)
    let proof = proof.0;
    channel.mix_felts(&proof.sampled_values.clone().flatten_cols());
    let random_coeff = channel.draw_secure_felt();
    if until == 5 {
        return Ok(bits);
    }
    // ---- 6: fri_commit
    let lifting_log_size = commitment_scheme.trees.last().unwrap().height;
    let bound = CirclePolyDegreeBound::new(lifting_log_size - commitment_scheme.config.fri_config.log_blowup_factor);
    let mut fri_verifier = FriVerifier::<MerkleChannel>::commit(channel, commitment_scheme.config.fri_config, proof.fri_proof, bound)
        .map_err(|_| VerifyError::InvalidProof)?;
    if until == 6 {
        return Ok(bits);
    }
    // ---- 7: fri_pow_and_queries
    if !channel.verify_pow_nonce(commitment_scheme.config.fri_config.pow_bits, proof.proof_of_work) {
        return Err(VerifyError::InvalidProof);
    }
    channel.mix_u64(proof.proof_of_work);
    let query_positions = fri_verifier.sample_query_positions(channel);
    let preprocessed_query_positions =
        prepare_preprocessed_query_positions(&query_positions, lifting_log_size, commitment_scheme.trees[0].height);
    if until == 7 {
        return Ok(bits);
    }
    // ---- 8: merkle_trace_trees
    let query_positions_tree = TreeVec::new(
        commitment_scheme
            .trees
            .iter()
            .enumerate()
            .map(|(i, _)| if i == 0 { preprocessed_query_positions.as_slice() } else { query_positions.as_slice() })
            .collect::<alloc::vec::Vec<_>>(),
    );
    commitment_scheme
        .trees
        .as_ref()
        .zip_eq(proof.decommitments)
        .zip_eq(proof.queried_values.clone())
        .zip_eq(query_positions_tree)
        .map(|(((tree, decommitment), queried_values), query_positions)| tree.verify(query_positions, queried_values, decommitment))
        .0
        .into_iter()
        .collect::<Result<(), _>>()
        .map_err(|_| VerifyError::InvalidProof)?;
    if until == 8 {
        return Ok(bits);
    }
    // ---- 9: fri_answers
    let samples = sample_points.zip_cols(proof.sampled_values).map_cols(|(sampled_points, sampled_values)| {
        zip(sampled_points, sampled_values).map(|(point, value)| PointSample { point, value }).collect_vec()
    });
    let column_log_sizes = commitment_scheme.trees.as_ref().map(|tree| tree.column_log_sizes.clone());
    let answers = fri_answers(column_log_sizes, samples, random_coeff, &query_positions, proof.queried_values, lifting_log_size)
        .map_err(|_| VerifyError::InvalidProof)?;
    if until == 9 {
        return Ok(bits);
    }
    // ---- 10: fri_decommit
    fri_verifier.decommit(answers).map_err(|_| VerifyError::InvalidProof)?;
    Ok(bits)
}
