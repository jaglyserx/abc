# Banyan Conformance Gap Matrix

Status legend:
- `implemented`: behavior exists and has direct test coverage.
- `partial`: behavior exists but is incomplete or not fully spec-faithful.
- `missing`: not implemented.

## Validation Rules
| Rule ID | Requirement | Status | Current Code | Test Evidence | Next Action |
|---|---|---|---|---|---|
| VAL-001 | Verify vote signatures before acceptance | implemented | `src/consensus.rs` (`verify_vote_signature`, vote handlers) | consensus tests pass (`cargo test`) | Keep in conformance suite with explicit rule-id tests |
| VAL-002 | Verify certificate/proof signatures and quorum | implemented | `src/consensus.rs` (`valid_notarization_certificate`, `valid_unlock_proof`, `valid_finalization_certificate`) | malformed notarization/unlock/finalization tests | Keep rule-id naming in dedicated conformance tests |
| VAL-003 | Canonical serialization/signing domain vectors | implemented | `vote_digest` + deterministic key/signature vectors | explicit conformance vector tests | Keep vectors stable across protocol versioning changes |

## Round and Leader Rules
| Rule ID | Requirement | Status | Current Code | Test Evidence | Next Action |
|---|---|---|---|---|---|
| RND-001 | Rank-0 proposer must be round leader | implemented | `src/consensus.rs` (`valid_proposal`) | `non_leader_rank_zero_proposal_is_rejected` | Add positive test for leader rank-0 under timeout-free path |
| RND-002 | Non-leader proposals require timeout path | partial | `src/consensus.rs` (`valid_proposal`, `round_timed_out`) | timeout advancement test exists | Add end-to-end timeout fallback proposal test |
| RND-003 | Round transition after timeout | implemented | `src/consensus.rs` (`on_tick`) | `timeout_advances_round` | Add multi-round liveness simulation assertion |

## Voting and Equivocation Rules
| Rule ID | Requirement | Status | Current Code | Test Evidence | Next Action |
|---|---|---|---|---|---|
| VOTE-001 | Reject equivocation for notarization votes | implemented | `src/consensus.rs`, `VotePools` per-voter-round map | `rejects_equivocating_notarization_vote` | Add same check for fast/finalization with dedicated tests |
| VOTE-002 | Reject equivocation for fast/finalization votes | implemented | `src/consensus.rs`, `VotePools` maps exist | explicit fast/finalization equivocation tests | Keep in conformance suite |
| VOTE-003 | One notarization vote per local node per round | implemented | `src/consensus.rs` (`voted_notarization_rounds`) | proposal tests | Add explicit regression test with two proposals same round |

## Finalization Safety Rules
| Rule ID | Requirement | Status | Current Code | Test Evidence | Next Action |
|---|---|---|---|---|---|
| FIN-001 | Do not finalize without valid finalization certificate | implemented | `src/consensus.rs` (`on_finalization`) | certificate validation path | Add invalid-finalization-cert negative test |
| FIN-002 | No conflicting finalized blocks in simulation | implemented | deterministic simulation includes hash conflict assertion | `deterministic_simulation_handles_delay_drop_and_byzantine` | Add broader multi-round invariant scenarios |

## Simulation and CI Gates
| Rule ID | Requirement | Status | Current Code | Test Evidence | Next Action |
|---|---|---|---|---|---|
| SIM-001 | Deterministic delayed/drop/byzantine simulation | implemented | `src/consensus.rs` tests | simulation test passes | Expand scenario matrix coverage |
| SIM-002 | Property-level invariants for safety/liveness | partial | targeted unit tests | current tests are scenario-based | Add property tests for no-conflicting-finalization |
| OPS-001 | CI runs conformance tests explicitly | implemented | `.github/workflows/ci.yml` has dedicated `conformance` job | CI job covers vectors/equivocation/certificate/simulation | Keep rule-to-job mapping maintained |

## Immediate Work Queue
1. Add timeout fallback positive-path test for non-leader ranked proposals.
2. Add broader multi-round and mixed-fault conformance scenarios.
3. Add external protocol review checklist/signoff artifact template.
