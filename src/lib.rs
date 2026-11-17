//! A small Solana RPC router.
//!
//! It tries a list of RPC nodes in order and moves on to the next one when a
//! node fails or is too slow. A node that fails is put on "cooldown" and
//! skipped until its cooldown timer runs out.
//!
//! `RpcRouter` is safe to share between tasks and threads: wrap it in an
//! `Arc` (`Arc::new(router)`) and clone the `Arc` wherever you need it.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use solana_client::nonblocking::rpc_client::RpcClient;

/// One RPC node and its cooldown timer.
struct RpcNode {
    url: String,
    client: RpcClient,
    /// While this time is in the future, the node is skipped.
    /// The Mutex lets many tasks read and update it safely.
    cooldown_until: Mutex<Option<Instant>>,
}

impl RpcNode {
    fn is_cooling_down(&self) -> bool {
        let cooldown_until = self.cooldown_until.lock().unwrap();
        match *cooldown_until {
            Some(until) => Instant::now() < until,
            None => false,
        }
    }

    fn start_cooldown(&self, duration: Duration) {
        let mut cooldown_until = self.cooldown_until.lock().unwrap();
        *cooldown_until = Some(Instant::now() + duration);
    }
}

pub struct RpcRouter {
    nodes: Vec<RpcNode>,
    /// Longest time to wait for a single node to answer.
    attempt_timeout: Duration,
    /// Longest time one whole `get_latest_slot()` call may take, across all nodes.
    total_deadline: Duration,
    /// How long a failed or timed-out node is skipped.
    cooldown_duration: Duration,
}

impl RpcRouter {
    /// Creates a router. Nodes are tried in the order given, so put your
    /// preferred (primary) node first.
    pub fn new(
        urls: &[&str],
        attempt_timeout: Duration,
        total_deadline: Duration,
        cooldown_duration: Duration,
    ) -> Self {
        let mut nodes = Vec::new();
        for url in urls {
            nodes.push(RpcNode {
                url: url.to_string(),
                client: RpcClient::new(url.to_string()),
                cooldown_until: Mutex::new(None),
            });
        }

        Self {
            nodes,
            attempt_timeout,
            total_deadline,
            cooldown_duration,
        }
    }

    /// Fetches the current slot from the first node that answers in time.
    ///
    /// Nodes on cooldown are skipped. A node that fails or takes longer than
    /// `attempt_timeout` is put on cooldown, and the next node is tried.
    /// Gives up once `total_deadline` has passed.
    pub async fn get_latest_slot(&self) -> Result<u64, String> {
        let started = Instant::now();
        let mut last_error = String::from("all nodes are cooling down");

        for node in &self.nodes {
            if node.is_cooling_down() {
                eprintln!("SKIP: {} is cooling down", node.url);
                continue;
            }

            // How much of the total time budget is left?
            let time_left = self.total_deadline.saturating_sub(started.elapsed());
            if time_left.is_zero() {
                return Err(format!(
                    "total deadline of {:?} reached ({last_error})",
                    self.total_deadline
                ));
            }

            // Wait no longer than one attempt is allowed, and no longer than
            // the time left in the total budget.
            let wait = self.attempt_timeout.min(time_left);

            match tokio::time::timeout(wait, node.client.get_slot()).await {
                Ok(Ok(slot)) => return Ok(slot),
                Ok(Err(err)) => last_error = format!("{} failed: {err}", node.url),
                Err(_) => last_error = format!("{} timed out after {wait:?}", node.url),
            }

            eprintln!("WARN: {last_error}. Trying the next node.");
            node.start_cooldown(self.cooldown_duration);
        }

        Err(last_error)
    }
}
