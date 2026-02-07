# Banyan Conformance Target

## Scope
This document defines what this repository must satisfy before claiming Banyan protocol conformance.

Conformance in this project means:
- Consensus transitions are spec-faithful for the chosen Banyan profile.
- Message validity and certificate validity rules are complete.
- Safety/liveness assumptions are explicitly modeled and tested.
- CI includes deterministic conformance tests mapped to rule IDs.

## Version Pin
- Protocol family: Banyan / ICC-derived round-based BFT.
- Project profile: practical implementation profile for this repository.
- Conformance authority in this repo: `docs/conformance/GAP_MATRIX.md`.

## Assumptions and Fault Model
- `n` validators per committee.
- Byzantine threshold `f` and fast-path threshold `p` configured at runtime.
- Partial synchrony: timeout-driven progress after network stabilizes.
- Network may delay, drop, reorder, and duplicate messages.

## Rule Categories
- `VAL-*`: message/certificate/proof validity.
- `RND-*`: round/leader/rank/timeout progression.
- `VOTE-*`: voting discipline and equivocation constraints.
- `FIN-*`: notarization/finalization safety properties.
- `SIM-*`: deterministic simulation scenarios and invariants.
- `OPS-*`: conformance gates in CI and release process.

## Required Invariants
- `INV-SAFETY-1`: No conflicting finalized blocks at same height/round.
- `INV-SAFETY-2`: Certificate/proof acceptance requires valid signatures and quorums.
- `INV-SAFETY-3`: A validator cannot cast conflicting votes in same round/type.
- `INV-LIVENESS-1`: Under partial synchrony and bounded faults, rounds eventually advance.
- `INV-LIVENESS-2`: Valid proposals can be notarized/finalized under non-adversarial schedules.

## Conformance Exit Criteria
All conditions must hold:
1. All rules in `GAP_MATRIX.md` are `implemented`.
2. Every rule has at least one deterministic test case.
3. Invariant suite passes in CI on every PR.
4. Conformance CI artifacts are published and retained for audit.
5. External review checklist is completed (`docs/conformance/EXTERNAL_REVIEW_CHECKLIST.md`).
6. No known safety deviations listed as open.
7. Release notes include conformance evidence and test artifact links.

## Notes
- This file defines target behavior; implementation details and current status live in `GAP_MATRIX.md`.
