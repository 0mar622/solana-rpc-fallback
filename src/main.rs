use solana_client::client_error::Result as ClientResult;
use solana_client::nonblocking::rpc_client::RpcClient;

const DEFAULT_PRIMARY_URL: &str = "http://127.0.0.1:8899";
const DEFAULT_BACKUP_URL: &str = "http://127.0.0.1:8900";

pub struct RpcRouter {
    pub primary_client: RpcClient,
    pub backup_client: RpcClient,
}

impl RpcRouter {
    pub fn new(primary_url: impl Into<String>, backup_url: impl Into<String>) -> Self {
        Self {
            primary_client: RpcClient::new(primary_url.into()),
            backup_client: RpcClient::new(backup_url.into()),
        }
    }

    /// Fetches the current slot from the primary node. If that fails, logs a
    /// warning and tries the backup node instead.
    pub async fn get_latest_slot(&self) -> ClientResult<u64> {
        match self.primary_client.get_slot().await {
            Ok(slot) => Ok(slot),
            Err(err) => {
                eprintln!(
                    "WARN: primary RPC ({}) failed: {err}. Falling back to backup ({}).",
                    self.primary_client.url(),
                    self.backup_client.url(),
                );
                self.backup_client.get_slot().await
            }
        }
    }
}

impl Default for RpcRouter {
    fn default() -> Self {
        Self::new(DEFAULT_PRIMARY_URL, DEFAULT_BACKUP_URL)
    }
}

// Public mainnet RPC endpoints (no API key needed). Note that website URLs
// like https://solana.com are not RPC nodes and will fail.
const SOLANA_MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";
const PUBLICNODE_RPC: &str = "https://solana-rpc.publicnode.com";

async fn run_scenario(name: &str, router: RpcRouter) {
    println!("\n=== {name} ===");
    println!(
        "primary: {}  |  backup: {}",
        router.primary_client.url(),
        router.backup_client.url()
    );
    match router.get_latest_slot().await {
        Ok(slot) => println!("OK: latest slot = {slot}"),
        Err(err) => eprintln!("ERROR: both RPC nodes failed: {err}"),
    }
}

#[tokio::main]
async fn main() {
    // 1. Both nodes work: should succeed on the primary, no warning.
    run_scenario(
        "Scenario 1: two working nodes",
        RpcRouter::new(SOLANA_MAINNET_RPC, PUBLICNODE_RPC),
    )
    .await;

    // 2. Primary is down: should warn, then get the slot from the backup.
    run_scenario(
        "Scenario 2: broken primary, working backup",
        RpcRouter::new(DEFAULT_PRIMARY_URL, SOLANA_MAINNET_RPC),
    )
    .await;

    // 3. Both are down: should warn, then return the backup's error.
    run_scenario(
        "Scenario 3: both nodes broken",
        RpcRouter::default(),
    )
    .await;
}
