# Operations Playbook

## Runtime Configuration
- `ABC_NODE_ID`: unique node ID.
- `ABC_DATA_DIR`: persistent root.
- `ABC_P2P_BIND`: gossip bind address.
- `ABC_P2P_PEERS`: comma-separated peer list.
- `ABC_ENABLE_FAUCET`: `true`/`false`.
- `ABC_MAX_FAUCET_AMOUNT`: upper faucet bound.
- `ABC_MIN_ACCOUNT_PASS_LEN`: minimum account passphrase length.

## Startup
1. Set environment variables for node identity and data directory.
2. Run `cargo run`.
3. Verify health with JSON-RPC `health` and `metrics` methods.

## Health Checks
- `health` returns uptime, current height, and mempool size.
- `metrics` returns RPC totals, tx and block counters, and snapshot sync counters.

## Backup and Restore
- Snapshot file: `data/node-<id>/state/state_snapshot.json`
- Block history: `data/node-<id>/state/blocks.jsonl`
- Backup both files atomically per checkpoint window.

Restore:
1. Stop the node.
2. Replace snapshot/block files in state directory.
3. Start node; startup rehydrates ledger from snapshot.

## Manual Catch-Up
1. On a healthy peer, call `export_snapshot` and `export_blocks`.
2. On the lagging node, call `import_snapshot` with exported payload.
3. Verify `health.current_height` and latest receipts.

## Key Management Hardening
- Account keyfiles are written encrypted and forced to mode `0600` on Unix.
- Use long passphrases and rotate account credentials periodically.
- In production, disable faucet (`ABC_ENABLE_FAUCET=false`).

## Incident Response
- If DB corruption suspected:
  1. Stop node.
  2. Archive current state files.
  3. Re-import from trusted peer snapshot.
- If nonce errors spike:
  1. Inspect `get_nonce` for affected accounts.
  2. Audit client submission logic for replay/ordering bugs.

## Testnet Deployment Checklist
- Enable CI status gates (fmt, clippy, tests, simulation).
- Pin config values per node and keep peer list consistent.
- Set faucet limit for testnet economics.
- Confirm backups are running and periodically validated.
