is having this enough:

# solana-rpc-fallback

A lightweight, asynchronous Rust RPC failover router (`RpcRouter`) for Solana. It wraps primary and backup `RpcClient` endpoints and automatically switches to the backup node if the primary fails or times out.

## Run the Prototype

```bash
cargo run
```

This runs three built-in test scenarios demonstrating normal routing, automatic primary-to-backup recovery, and full outage error handling.
