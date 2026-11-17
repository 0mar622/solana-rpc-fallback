use std::time::Duration;

use solana_rpc_fallback::RpcRouter;

#[tokio::main]
async fn main() {
    // The first node is a local one that isn't running, so the router
    // should warn, put it on cooldown, and use the mainnet node instead.
    let router = RpcRouter::new(
        &[
            "http://127.0.0.1:8899",
            "https://api.mainnet-beta.solana.com",
            "https://solana-rpc.publicnode.com",
        ],
        Duration::from_secs(2),  // attempt_timeout
        Duration::from_secs(5),  // total_deadline
        Duration::from_secs(30), // cooldown_duration
    );

    // The second request should skip the local node, because it's on cooldown.
    for request in 1..=2 {
        println!("\n=== Request {request} ===");
        match router.get_latest_slot().await {
            Ok(slot) => println!("OK: latest slot = {slot}"),
            Err(err) => eprintln!("ERROR: {err}"),
        }
    }
}
