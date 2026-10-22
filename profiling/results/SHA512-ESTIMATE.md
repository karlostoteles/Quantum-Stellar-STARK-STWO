AIR sketch: 216 columns (192 bit columns at 3 mask points, 12 limbs, 12 carries), 2^7 rows, 608 sampled values at the OODS point, degree <= 3.

| FRI parameters | security (Stwo formula) | proof size (est.) | fits 132,096 B? | verification today (est.) | share of 400 M |
|---|---|---|---|---|---|
| compact: pow 24, blowup 2^8, 9 queries, fold 8 | 96 bits | ~24 KB | yes | ~141 M | 35 % |
| pow 24, blowup 2^8, 21 queries, fold 8 | 192 bits | ~41 KB | yes | ~202 M | 50 % |
| StarkWare default: pow 26, blowup 2, 70 queries | 96 bits | ~62 KB | yes | ~372 M | 93 % |

Compact parameters, by phase (CPU instructions, estimate):

| phase | estimate | method |
|---|---|---|
| decode | 6.4 M | measured cost per byte × estimated size |
| setup (component info pass) | 16.4 M | 4× the FibonacciSq pass (more and longer constraints) |
| mask points | 13.4 M | ∝ sampled values |
| composition evaluation | 19.1 M | 216 constraints × 20 QM31 mul × 4,424 |
| mix sampled values | 7.6 M | ∝ sampled values |
| Merkle, trace trees | 12.8 M | per query from the reference; 45 % ∝ columns, 55 % ∝ depth |
| quotients (fri_answers) | 51.0 M | a + b·queries fitted on both references, × sampled values (dominant) |
| FRI decommit | 13.7 M | per query×layer from the reference with the same fold step |
| other (commit, PoW, queries) | 0.7 M | ∝ queries |
| **total** | **141 M** | ~35 % of a transaction |

Reference per-unit costs used: QM31 mul 4,424 CPU, Blake2s block 21,880 CPU; phases from compact-phases.json and standard-phases.json; size factor measured/raw = 0.65 (9 queries) to 0.42 (70 queries).
