# Where the cost of verifying a Stwo proof on Soroban goes

Measured with soroban-env-host 29.0.1 (protocol 29 metering) and the deployed contract code (same build, function names kept). One wasmi fuel = 4 CPU instructions.

## Totals

| Proof | Proof size | CPU instructions (host total) | Wasm fuel | ×4 = Wasm CPU | Wasm calls executed | Native verify (x86, 2.8 GHz) |
|---|---|---|---|---|---|---|
| compact | 12,813 B | 43,126,433 | 10,522,993 | 42,091,972 | 32,595 | 224 µs |
| standard | 71,701 B | 216,570,208 | 53,051,879 | 212,207,516 | 121,512 | 1246 µs |
| compact-192 | 26,164 B | 73,715,193 | 17,980,424 | 71,921,696 | 48,641 | 398 µs |

## The `fast` build (PATCHES.md change 6) against the deployed code

| Proof | deployed: CPU | fast: CPU | change | deployed: Wasm calls | fast: calls | native µs (deployed → fast) |
|---|---|---|---|---|---|---|
| compact | 43,126,433 | 17,340,037 | -59.8 % | 32,595 | 4,007 | 224 → 178 |
| standard | 216,570,208 | 99,835,916 | -53.9 % | 121,512 | 17,953 | 1246 → 766 |
| compact-192 | 73,715,193 | 31,385,889 | -57.4 % | 48,641 | 5,991 | 398 → 250 |

By phase (fuel), deployed → fast:

| Phase | compact | standard | compact-192 |
|---|---|---|---|
| 0. decode | 840,197 → 797,853 (-5 %) | 4,120,368 → 3,912,497 (-5 %) | 1,548,344 → 1,469,503 (-5 %) |
| 1. setup_component | 1,023,690 → 8,019 (-99 %) | 1,023,690 → 8,019 (-99 %) | 1,024,884 → 8,019 (-99 %) |
| 2. commit_trace_trees | 37,232 → 27,557 (-26 %) | 37,232 → 27,557 (-26 %) | 37,232 → 27,557 (-26 %) |
| 3. oods_pow_and_point | 596,074 → 74,719 (-87 %) | 592,510 → 74,719 (-87 %) | 596,074 → 74,719 (-87 %) |
| 4. composition_eval | 647,495 → 235,210 (-64 %) | 678,377 → 246,607 (-64 %) | 647,495 → 235,210 (-64 %) |
| 5. mix_sampled_values | 336,427 → 259,427 (-23 %) | 335,233 → 259,427 (-23 %) | 335,233 → 260,612 (-22 %) |
| 6. fri_commit | 87,079 → 36,519 (-58 %) | 252,509 → 132,425 (-48 %) | 87,079 → 36,519 (-58 %) |
| 7. fri_pow_and_queries | 53,705 → 36,222 (-33 %) | 186,413 → 142,343 (-24 %) | 76,832 → 55,300 (-28 %) |
| 8. merkle_trace_trees | 1,923,734 → 1,018,740 (-47 %) | 10,389,454 → 5,518,969 (-47 %) | 4,107,097 → 2,171,590 (-47 %) |
| 9. fri_answers | 2,266,386 → 751,725 (-67 %) | 6,762,960 → 2,051,615 (-70 %) | 3,165,537 → 1,010,618 (-68 %) |
| 10. fri_decommit | 2,736,995 → 1,034,526 (-62 %) | 29,071,784 → 12,091,491 (-58 %) | 6,453,681 → 2,326,763 (-64 %) |

Top functions of the fast build, compact:

| self fuel | share | inclusive | calls | function |
|---|---|---|---|---|
| 1,257,984 | 30.8 % | 1,257,984 | 416 | `<blake2::Blake2sVarCore>::compress` |
| 637,029 | 15.6 % | 2,946,316 | 1 | `<pcs::verifier::CommitmentSchemeVerifier<vcs_lifted::blake2_merkle::Blake2sM31MerkleCha…` |
| 395,255 | 9.7 % | 4,079,399 | 0 | `verify` |
| 383,161 | 9.4 % | 1,509,163 | 5 | `<vcs_lifted::verifier::MerkleVerifierLifted<vcs::blake2_hash::Blake2sHasherGeneric<fals…` |
| 131,675 | 3.2 % | 131,675 | 229 | `postcard::de::deserializer::SeqAccess<postcard::de::flavors::Slice>::next_element::<vcs…` |
| 116,336 | 2.9 % | 131,539 | 661 | `__rustc::__rust_alloc` |
| 109,693 | 2.7 % | 131,782 | 114 | `postcard::de::deserializer::SeqAccess<postcard::de::flavors::Slice>::next_element::<all…` |
| 104,939 | 2.6 % | 104,939 | 101 | `stwo_soroban_air::FibSqFastComponent::evaluate_constraint_quotients_at_point::{closure#0}` |
| 82,168 | 2.0 % | 82,168 | 80 | `<circle::CirclePointIndex>::to_point` |
| 77,200 | 1.9 % | 77,200 | 200 | `fields::qm31::QM31::square` |
| 70,256 | 1.7 % | 117,478 | 2 | `fri::build_merkle_verification_inputs::<core::iter::adapters::copied::Copied<core::iter…` |
| 59,216 | 1.5 % | 98,512 | 16 | `fri::fold_coset` |

Top functions of the fast build, standard:

| self fuel | share | inclusive | calls | function |
|---|---|---|---|---|
| 9,921,744 | 41.5 % | 9,921,744 | 3,281 | `<blake2::Blake2sVarCore>::compress` |
| 3,122,103 | 13.1 % | 12,875,553 | 13 | `<vcs_lifted::verifier::MerkleVerifierLifted<vcs::blake2_hash::Blake2sHasherGeneric<fals…` |
| 2,522,994 | 10.6 % | 23,884,250 | 0 | `verify` |
| 2,066,869 | 8.7 % | 19,771,093 | 1 | `<pcs::verifier::CommitmentSchemeVerifier<vcs_lifted::blake2_merkle::Blake2sM31MerkleCha…` |
| 798,776 | 3.3 % | 798,776 | 794 | `<circle::CirclePointIndex>::to_point` |
| 709,548 | 3.0 % | 918,348 | 10 | `fri::build_merkle_verification_inputs::<core::iter::adapters::copied::Copied<core::iter…` |
| 664,950 | 2.8 % | 1,276,704 | 286 | `fri::fold_coset` |
| 657,225 | 2.8 % | 657,225 | 1,143 | `postcard::de::deserializer::SeqAccess<postcard::de::flavors::Slice>::next_element::<vcs…` |
| 472,159 | 2.0 % | 847,934 | 10 | `fri::compute_decommitment_positions_and_rebuild_evals::<&mut core::iter::adapters::copi…` |
| 391,776 | 1.6 % | 446,529 | 2,226 | `__rustc::__rust_alloc` |
| 362,100 | 1.5 % | 362,100 | 425 | `fields::m31::pow2147483645::<fields::m31::M31>` |
| 283,339 | 1.2 % | 937,790 | 14 | `alloc::vec::Vec<vcs::blake2_hash::Blake2sHash>::deserialize::<&mut postcard::de::deseri…` |

Fuel by instruction class, fast build:

| Proof | call (×67) share | call instructions charged | calls executed | total if call cost 1 |
|---|---|---|---|---|
| compact | 17.4 % | 10,618 | 4,007 | 3,378,611 (-17.2 %) |
| standard | 15.4 % | 54,890 | 17,953 | 20,261,510 (-15.2 %) |
| compact-192 | 16.2 % | 17,908 | 5,991 | 6,223,488 (-16.0 %) |

## By phase of the verifier (deployed code)

Cumulative cost after each phase (probe build, `verify_phase`), differenced. Fuel; share of the phased total.

| Phase | compact fuel | compact share | standard fuel | standard share | compact-192 fuel | compact-192 share |
|---|---|---|---|---|---|---|
| 0. decode | 840,197 | 8.0 % | 4,120,368 | 7.7 % | 1,548,344 | 8.6 % |
| 1. setup_component | 1,023,690 | 9.7 % | 1,023,690 | 1.9 % | 1,024,884 | 5.7 % |
| 2. commit_trace_trees | 37,232 | 0.4 % | 37,232 | 0.1 % | 37,232 | 0.2 % |
| 3. oods_pow_and_point | 596,074 | 5.7 % | 592,510 | 1.1 % | 596,074 | 3.3 % |
| 4. composition_eval | 647,495 | 6.1 % | 678,377 | 1.3 % | 647,495 | 3.6 % |
| 5. mix_sampled_values | 336,427 | 3.2 % | 335,233 | 0.6 % | 335,233 | 1.9 % |
| 6. fri_commit | 87,079 | 0.8 % | 252,509 | 0.5 % | 87,079 | 0.5 % |
| 7. fri_pow_and_queries | 53,705 | 0.5 % | 186,413 | 0.3 % | 76,832 | 0.4 % |
| 8. merkle_trace_trees | 1,923,734 | 18.2 % | 10,389,454 | 19.4 % | 4,107,097 | 22.7 % |
| 9. fri_answers | 2,266,386 | 21.5 % | 6,762,960 | 12.7 % | 3,165,537 | 17.5 % |
| 10. fri_decommit | 2,736,995 | 25.9 % | 29,071,784 | 54.4 % | 6,453,681 | 35.7 % |
| total (phased) | 10,549,014 | — | 53,450,530 | — | 18,079,488 | — |
| `verify` in the same build | 10,589,099 | | 53,513,435 | | 18,122,627 | |

## By kind of code (self fuel of the functions, deployed build)

Inlined callees count towards their caller, so these are lower bounds for leaf work and upper bounds for the big functions.

| Category | compact fuel | compact share | standard fuel | standard share | compact-192 fuel | compact-192 share |
|---|---|---|---|---|---|---|
| proof decoding (postcard/serde; the rest of it is inlined into `verify`) | 302,874 | 2.9 % | 1,139,380 | 2.1 % | 519,780 | 2.9 % |
| Merkle verifier, own code (hashing excluded) | 963,959 | 9.2 % | 7,651,853 | 14.4 % | 2,077,632 | 11.6 % |
| verifier glue: verify, verify_envelope, verify_values (quotients inlined here) | 1,488,096 | 14.1 % | 6,400,176 | 12.1 % | 2,479,536 | 13.8 % |
| Blake2s: compress and hasher wrapper | 1,397,192 | 13.3 % | 10,901,233 | 20.5 % | 2,854,101 | 15.9 % |
| twiddles and domain points (CirclePoint, Coset, domains) | 939,529 | 8.9 % | 7,027,512 | 13.2 % | 2,083,587 | 11.6 % |
| field arithmetic functions (CM31, QM31, M31 inverse) | 2,323,514 | 22.1 % | 7,890,224 | 14.9 % | 3,480,830 | 19.4 % |
| memcpy, memset, allocator | 2,275,994 | 21.6 % | 9,410,797 | 17.7 % | 3,403,046 | 18.9 % |
| constraint framework (component info pass, constraint evaluation) | 536,713 | 5.1 % | 537,271 | 1.0 % | 536,713 | 3.0 % |
| FRI, own code (folding glue, witness rebuild) | 167,228 | 1.6 % | 1,886,058 | 3.6 % | 409,472 | 2.3 % |
| sorting, hash maps, iterators | 85,677 | 0.8 % | 110,372 | 0.2 % | 90,698 | 0.5 % |
| channel (Fiat-Shamir) | 22,897 | 0.2 % | 50,776 | 0.1 % | 23,802 | 0.1 % |
| everything else | 19,320 | 0.2 % | 46,227 | 0.1 % | 21,227 | 0.1 % |

### Top functions, compact (self fuel, inclusive fuel, calls)

| self fuel | share | inclusive | calls | function |
|---|---|---|---|---|
| 1,293,870 | 12.3 % | 1,293,870 | 12,685 | `fields::cm31::CM31::mul` |
| 1,257,984 | 12.0 % | 1,257,984 | 416 | `<blake2::Blake2sVarCore>::compress` |
| 1,139,035 | 10.8 % | 1,139,035 | 2,215 | `memcpy` |
| 987,712 | 9.4 % | 7,392,713 | 1 | `<pcs::verifier::CommitmentSchemeVerifier<vcs_lifted::blake2_merkle::Blake2sM31MerkleCha…` |
| 956,598 | 9.1 % | 2,944,254 | 5 | `<vcs_lifted::verifier::MerkleVerifierLifted<vcs::blake2_hash::Blake2sHasherGeneric<fals…` |
| 816,114 | 7.8 % | 1,297,146 | 246 | `<circle::CirclePoint<fields::m31::M31>>::mul` |
| 781,200 | 7.4 % | 1,584,450 | 1,575 | `fields::qm31::QM31::mul` |
| 395,376 | 3.8 % | 10,522,993 | 0 | `verify` |
| 305,551 | 2.9 % | 305,551 | 1,211 | `memset` |
| 278,432 | 2.6 % | 316,012 | 1,582 | `__rustc::__rust_alloc` |
| 176,704 | 1.7 % | 176,704 | 2,008 | `fields::qm31::QM31::mul` |
| 159,480 | 1.5 % | 1,298,515 | 2,215 | `memcpy` |
| 131,675 | 1.3 % | 131,675 | 229 | `postcard::de::deserializer::SeqAccess<postcard::de::flavors::Slice>::next_element::<vcs…` |
| 115,460 | 1.1 % | 496,646 | 502 | `<stwo_constraint_framework::info::ArithmeticCounter<false>>::drain` |
| 109,693 | 1.0 % | 131,782 | 114 | `postcard::de::deserializer::SeqAccess<postcard::de::flavors::Slice>::next_element::<all…` |
| 101,422 | 1.0 % | 1,313,922 | 314 | `<vcs::blake2_hash::Blake2sHasherGeneric<false>>::finalize` |
| 101,087 | 1.0 % | 9,739,695 | 1 | `stwo_soroban_air::verify_envelope` |
| 96,960 | 0.9 % | 522,789 | 101 | `stwo_constraint_framework::info::InfoEvaluator::add_constraint::<stwo_constraint_framew…` |
| 87,192 | 0.8 % | 392,743 | 1,211 | `memset` |
| 85,035 | 0.8 % | 85,185 | 109 | `core::iter::adapters::flatten::Flatten<core::iter::adapters::flatten::Flatten<alloc::ve…` |

### Top functions, standard (self fuel, inclusive fuel, calls)

| self fuel | share | inclusive | calls | function |
|---|---|---|---|---|
| 9,921,744 | 18.7 % | 9,921,744 | 3,281 | `<blake2::Blake2sVarCore>::compress` |
| 7,281,128 | 13.7 % | 25,167,898 | 13 | `<vcs_lifted::verifier::MerkleVerifierLifted<vcs::blake2_hash::Blake2sHasherGeneric<fals…` |
| 6,714,045 | 12.7 % | 10,618,299 | 1,250 | `<circle::CirclePoint<fields::m31::M31>>::mul` |
| 5,566,048 | 10.5 % | 5,566,048 | 8,393 | `memcpy` |
| 5,089,392 | 9.6 % | 5,089,392 | 49,896 | `fields::cm31::CM31::mul` |
| 3,771,243 | 7.1 % | 46,760,065 | 1 | `<pcs::verifier::CommitmentSchemeVerifier<vcs_lifted::blake2_merkle::Blake2sM31MerkleCha…` |
| 2,523,115 | 4.8 % | 53,051,879 | 0 | `verify` |
| 1,357,303 | 2.6 % | 1,357,303 | 4,593 | `memset` |
| 1,342,704 | 2.5 % | 1,342,704 | 15,258 | `fields::qm31::QM31::mul` |
| 1,065,904 | 2.0 % | 2,161,894 | 2,149 | `fields::qm31::QM31::mul` |
| 903,108 | 1.7 % | 11,859,426 | 2,796 | `<vcs::blake2_hash::Blake2sHasherGeneric<false>>::finalize` |
| 709,548 | 1.3 % | 1,052,997 | 10 | `fri::build_merkle_verification_inputs::<core::iter::adapters::copied::Copied<core::iter…` |
| 657,225 | 1.2 % | 657,225 | 1,143 | `postcard::de::deserializer::SeqAccess<postcard::de::flavors::Slice>::next_element::<vcs…` |
| 604,296 | 1.1 % | 6,170,344 | 8,393 | `memcpy` |
| 576,400 | 1.1 % | 662,471 | 3,275 | `__rustc::__rust_alloc` |
| 497,299 | 0.9 % | 911,991 | 10 | `fri::compute_decommitment_positions_and_rebuild_evals::<&mut core::iter::adapters::copi…` |
| 363,804 | 0.7 % | 363,804 | 427 | `fields::m31::pow2147483645::<fields::m31::M31>` |
| 330,696 | 0.6 % | 1,687,999 | 4,593 | `memset` |
| 283,339 | 0.5 % | 937,799 | 14 | `alloc::vec::Vec<vcs::blake2_hash::Blake2sHash>::deserialize::<&mut postcard::de::deseri…` |
| 250,536 | 0.5 % | 3,886,200 | 286 | `fri::fold_coset` |

## By instruction class (wasmi fuel weights: base 1, entity 3, load 2, store 1, call 67)

| Proof | base | entity (×3) | load (×2) | store | call (×67) | locals/branch term | total | call instructions charged | calls executed | total if call cost 1 |
|---|---|---|---|---|---|---|---|---|---|---|
| compact | 59.8 % | 0.5 % | 5.0 % | 2.3 % | 32.2 % | 0.2 % | 10,522,993 | 50,633 | 32,595 | 7,181,215 (-31.8 %) |
| standard | 64.0 % | 0.3 % | 4.8 % | 2.2 % | 28.5 % | 0.2 % | 53,051,879 | 225,931 | 121,512 | 38,140,433 (-28.1 %) |
| compact-192 | 61.7 % | 0.4 % | 4.9 % | 2.2 % | 30.6 % | 0.2 % | 17,980,424 | 82,051 | 48,641 | 12,565,058 (-30.1 %) |

wasmi charges a block's fuel when the block is entered, so instructions after a taken branch inside the block are paid for but not executed: that is the gap between call instructions charged and calls executed.

## Cost per operation (micro-benchmarks, `bench` entry point, same build flags)

| Operation | Wasm fuel per op | CPU per op (host costs included) | Wasm calls per op |
|---|---|---|---|
| loop only | 19 | 76 | 0.0 |
| M31 add | 19 | 76 | 0.0 |
| M31 mul | 26 | 104 | 0.0 |
| M31 inverse | 939 | 3,756 | 1.0 |
| CM31 mul | 191 | 764 | 1.0 |
| QM31 add | 19 | 76 | 0.0 |
| QM31 mul | 1106 | 4,424 | 6.0 |
| QM31 mul by M31 | 186 | 744 | 1.0 |
| QM31 inverse | 2103 | 8,412 | 7.0 |
| Blake2s of 64 B (one compression) | 5470 | 21,880 | 7.0 |
| circle point by scalar (a FRI twiddle) | 11183 | 44,730 | 49.3 |
| bn254_fr_mul through the host | 526 | 12,842 | 1.0 |
| bn254_fr_add through the host | 526 | 11,347 | 1.0 |
| sha256 of 64 B through the host | 504 | 17,402 | 0.0 |

`loop only` is the loop overhead to subtract from the cheap operations.

## Operations executed (native run of the same verifier, `count-ops`)

- compact: M31 mul 62,035, add 28,736, sub 19,141, inverse 83; CM31 mul 7,919; QM31 mul 1,575, inverse 2; Blake2s hashes 335 (24,771 bytes, 416 compressions).
- standard: M31 mul 276,951, add 122,633, sub 83,602, inverse 433; CM31 mul 11,029; QM31 mul 2,149, inverse 2; Blake2s hashes 2,840 (169,710 bytes, 3,281 compressions).
- compact-192: M31 mul 106,539, add 48,070, sub 32,319, inverse 191; CM31 mul 8,687; QM31 mul 1,719, inverse 2; Blake2s hashes 704 (50,856 bytes, 857 compressions).

Per phase, compact proof:

| phase | M31 mul | M31 add+sub | M31 inv | CM31 mul | QM31 mul | Blake2s hashes | bytes hashed | compressions |
|---|---|---|---|---|---|---|---|---|
| commitments_pow | 152 | 0 | 4 | 0 | 0 | 8 | 417 | 8 |
| oods_mask_points | 8,345 | 6,583 | 1 | 2,019 | 403 | 1 | 37 | 1 |
| composition_eval | 9,109 | 7,469 | 5 | 2,144 | 428 | 0 | 0 | 0 |
| mix_sampled_values | 0 | 0 | 0 | 0 | 0 | 2 | 1,797 | 29 |
| fri_commit | 544 | 302 | 0 | 0 | 0 | 5 | 250 | 5 |
| pow_queries | 0 | 0 | 0 | 0 | 0 | 5 | 206 | 5 |
| merkle_trace_trees | 0 | 0 | 0 | 0 | 0 | 172 | 13,744 | 226 |
| fri_answers | 23,065 | 23,093 | 9 | 3,276 | 648 | 0 | 0 | 0 |
| fri_first_layer | 0 | 0 | 0 | 0 | 0 | 75 | 4,800 | 75 |
| fri_inner_layers | 19,736 | 9,888 | 64 | 480 | 96 | 67 | 3,520 | 67 |
| fri_last_layer | 1,084 | 542 | 0 | 0 | 0 | 0 | 0 | 0 |


## What each lever would save (CPU instructions; baseline = host total for the deployed contract)

| Lever | kind | compact | standard | compact-192 |
|---|---|---|---|---|
| baseline | measured | 43.1 M | 216.6 M | 73.7 M |
| A. Wasm `call` charged 1 fuel instead of 67 | protocol (fuel model), exact | −13.4 M (-31.0 %) | −59.6 M (-27.5 %) | −21.7 M (-29.4 %) |
| C. verifier engineering, total (C1–C5) | no protocol change, estimate | −19.8 M (-46.0 %) | −91.3 M (-42.2 %) | −33.4 M (-45.3 %) |
| &nbsp;&nbsp;C1. twiddles from a table instead of a scalar multiplication per butterfly | estimate (keeps 10 %) | −4.7 M (-10.8 %) | −38.2 M (-17.7 %) | −11.0 M (-15.0 %) |
| &nbsp;&nbsp;C2. field arithmetic inlined (its `call` charges) | exact lower bound | −4.4 M (-10.2 %) | −18.2 M (-8.4 %) | −7.2 M (-9.7 %) |
| &nbsp;&nbsp;C3. half of the copying and allocation | estimate | −4.6 M (-10.6 %) | −18.9 M (-8.7 %) | −6.8 M (-9.3 %) |
| &nbsp;&nbsp;C4. component info pass done at compile time | estimate (keeps 10 %) | −3.7 M (-8.5 %) | −3.7 M (-1.7 %) | −3.7 M (-5.0 %) |
| &nbsp;&nbsp;C5. fixed-layout proof format | estimate (keeps 25 %) | −2.5 M (-5.8 %) | −12.4 M (-5.7 %) | −4.6 M (-6.3 %) |
| D. Merkle path verification host function, hash priced like SHA-256 | protocol (new host function) | −9.4 M (-21.9 %) | −87.0 M (-40.2 %) | −21.2 M (-28.7 %) |
| &nbsp;&nbsp;D'. same, Blake2s priced at half of SHA-256 | protocol | −10.6 M (-24.6 %) | −96.6 M (-44.6 %) | −23.7 M (-32.2 %) |
| E. hash host function, one call per hash (today's ABI, measured price) | protocol | +0.3 M (+0.6 %) | +5.9 M (+2.7 %) | +0.9 M (+1.2 %) |
| &nbsp;&nbsp;E'. hash host function over many inputs per call | protocol | −3.0 M (-6.9 %) | −23.9 M (-11.0 %) | −6.0 M (-8.2 %) |
| F. per-element M31/QM31 host functions, today's ABI (bn254_fr_* price for each QM31 mul) | protocol, measured price | +13.3 M (+30.7 %) | +18.1 M (+8.4 %) | +14.5 M (+19.6 %) |
| &nbsp;&nbsp;F'. per-element, ideal ABI (dispatch + call + 100), QM31 mul and inverses only | protocol, upper bound | −6.2 M (-14.4 %) | −9.4 M (-4.4 %) | −7.1 M (-9.6 %) |
| G. quotient accumulation as one host call per query | protocol | −9.0 M (-20.9 %) | −26.5 M (-12.2 %) | −12.5 M (-17.0 %) |
| H. the whole Stwo verifier as a host function (native time priced like ed25519) | protocol, order of magnitude | −41.4 M to −42.2 M (-96 to -98 %) | −207.2 M to −211.6 M (-96 to -98 %) | −70.7 M to −72.1 M (-96 to -98 %) |

Inputs behind D and E, per proof: Merkle verification inclusive (fuel×4) compact 11.8 M (host price 2.4 M); standard 106.5 M (host price 19.5 M); compact-192 26.3 M (host price 5.2 M). Blake2s in Wasm: compact 5.6 M; standard 43.5 M; compact-192 11.4 M.


## The protocol levers against the `fast` build (what is left for the host to save after the verifier is tuned)

| Lever | kind | compact | standard | compact-192 |
|---|---|---|---|---|
| baseline | measured | 17.3 M | 99.8 M | 31.4 M |
| A. Wasm `call` charged 1 fuel instead of 67 | protocol (fuel model), exact | −2.8 M (-16.2 %) | −14.5 M (-14.5 %) | −4.7 M (-15.1 %) |
| D. Merkle path verification host function, hash priced like SHA-256 | protocol (new host function) | −3.7 M (-21.2 %) | −32.2 M (-32.3 %) | −8.1 M (-25.8 %) |
| &nbsp;&nbsp;D'. same, Blake2s priced at half of SHA-256 | protocol | −4.9 M (-28.0 %) | −41.8 M (-41.9 %) | −10.7 M (-33.9 %) |
| &nbsp;&nbsp;E'. hash host function over many inputs per call | protocol | −2.5 M (-14.6 %) | −20.2 M (-20.2 %) | −5.1 M (-16.2 %) |
| G. quotient accumulation as one host call per query | protocol | −2.9 M (-16.9 %) | −7.7 M (-7.7 %) | −3.9 M (-12.4 %) |
| H. the whole Stwo verifier as a host function (native time priced like ed25519) | protocol, order of magnitude | −16.0 M to −16.6 M (-92 to -96 %) | −94.1 M to −96.8 M (-94 to -97 %) | −29.5 M to −30.4 M (-94 to -97 %) |

