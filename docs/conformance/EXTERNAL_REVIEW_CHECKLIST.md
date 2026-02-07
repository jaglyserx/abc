# External Banyan Conformance Review Checklist

Use this checklist for independent protocol review before claiming Banyan conformance.

## Review Metadata
- Review date (UTC):
- Reviewer name:
- Reviewer organization:
- Repository commit SHA:
- Protocol profile/version:
- Environment:
  - Rust toolchain:
  - OS/arch:

## Scope Confirmation
- [ ] `docs/conformance/BANYAN_CONFORMANCE.md` reviewed and accepted as the target.
- [ ] `docs/conformance/GAP_MATRIX.md` has no `missing` or `partial` rules for release.
- [ ] Conformance CI job passes on the reviewed commit.

## Rule Coverage Verification
- [ ] `VAL-*` rules reviewed against implementation and tests.
- [ ] `RND-*` rules reviewed against implementation and tests.
- [ ] `VOTE-*` rules reviewed against implementation and tests.
- [ ] `FIN-*` rules reviewed against implementation and tests.
- [ ] `SIM-*` rules reviewed against implementation and tests.
- [ ] `OPS-*` rules reviewed for CI/release gate coverage.

## Safety Findings
- [ ] No conflicting finalization scenario found in provided deterministic or property tests.
- [ ] Certificate/proof validation enforces signer uniqueness, quorum, and signature verification.
- [ ] Equivocation guards reject conflicting votes by the same validator/round/type.

Notes:

## Liveness Findings
- [ ] Timeout path exists and is exercised by tests.
- [ ] Round advancement under delayed/reordered network is demonstrated by simulations.
- [ ] No obvious deadlock condition identified in reviewed scope.

Notes:

## Artifact Validation
- [ ] Conformance CI artifacts contain logs for all conformance steps.
- [ ] Artifact names include enough context to map to rule/test IDs.
- [ ] Artifact retention policy is acceptable for audit needs.

## Open Issues / Deviations
- [ ] None

If issues exist, list each with severity and blocking status:
1.

## Signoff
- Review result:
  - [ ] Approved for conformance claim
  - [ ] Approved with non-blocking follow-ups
  - [ ] Not approved
- Reviewer signature/name:
- Date (UTC):
