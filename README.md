# solana-rpc-fallback

A lightweight, asynchronous Rust RPC failover router (`RpcRouter`) for Solana. It wraps multiple `RpcClient` endpoints and automatically routes around frozen, timed-out, or failing nodes using per-attempt timeouts, total request deadlines, and per-node cooldown timers.

## Run Automated Tests

```bash
cargo test
```

Runs local TCP stub-server integration tests (`tests/failover.rs`) verifying:
1. Failing over within the total deadline when a primary node accepts a connection but hangs without replying.
2. Skipping a failed primary immediately on subsequent requests while its cooldown timer is active.
3. Automatically returning traffic to the primary node once its cooldown expires.

## Run Live Demo

```bash
cargo run
```

Demonstrates a failed primary node triggering automatic failover on Request 1, followed by an instant cooldown skip on Request 2 against live Solana mainnet.
