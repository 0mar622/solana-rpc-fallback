//! Tests for RpcRouter using fake RPC nodes on local ports.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use solana_rpc_fallback::RpcRouter;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// ---------- A fake RPC node ----------

/// A fake Solana RPC node running on a random local port.
struct MockNode {
    url: String,
    /// How many requests reached this node.
    hits: Arc<AtomicUsize>,
    /// When false, the node accepts the connection but never replies.
    healthy: Arc<AtomicBool>,
}

impl MockNode {
    /// Starts a fake node that answers `getSlot` with `slot`.
    async fn start(slot: u64, healthy: bool) -> MockNode {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let healthy = Arc::new(AtomicBool::new(healthy));

        // Accept connections in the background, one task per connection.
        let hits_for_server = hits.clone();
        let healthy_for_server = healthy.clone();
        tokio::spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                tokio::spawn(handle_connection(
                    socket,
                    slot,
                    healthy_for_server.clone(),
                    hits_for_server.clone(),
                ));
            }
        });

        MockNode { url, hits, healthy }
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    fn set_healthy(&self, healthy: bool) {
        self.healthy.store(healthy, Ordering::SeqCst);
    }
}

async fn handle_connection(
    mut socket: TcpStream,
    slot: u64,
    healthy: Arc<AtomicBool>,
    hits: Arc<AtomicUsize>,
) {
    if !read_http_request(&mut socket).await {
        return; // client closed the connection without sending a request
    }
    hits.fetch_add(1, Ordering::SeqCst);

    if !healthy.load(Ordering::SeqCst) {
        // Simulate a stuck node: keep the connection open and never answer.
        std::future::pending::<()>().await;
    }

    let body = format!(r#"{{"jsonrpc":"2.0","result":{slot},"id":1}}"#);
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = socket.write_all(response.as_bytes()).await;
}

/// Reads one full HTTP request (headers + body). Returns false if the
/// connection closed first.
async fn read_http_request(socket: &mut TcpStream) -> bool {
    let mut data = Vec::new();
    let mut buffer = [0u8; 1024];

    loop {
        let bytes_read = socket.read(&mut buffer).await.unwrap_or(0);
        if bytes_read == 0 {
            return false;
        }
        data.extend_from_slice(&buffer[..bytes_read]);

        let text = String::from_utf8_lossy(&data);
        if let Some(headers_end) = text.find("\r\n\r\n") {
            let mut content_length = 0;
            for line in text[..headers_end].lines() {
                let line = line.to_lowercase();
                if let Some(value) = line.strip_prefix("content-length:") {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
            if data.len() >= headers_end + 4 + content_length {
                return true;
            }
        }
    }
}

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

// ---------- Tests ----------

const PRIMARY_SLOT: u64 = 100;
const BACKUP_SLOT: u64 = 200;

#[tokio::test]
async fn fails_over_within_deadline_when_primary_hangs() {
    let primary = MockNode::start(PRIMARY_SLOT, false).await; // hangs
    let backup = MockNode::start(BACKUP_SLOT, true).await;

    let router = RpcRouter::new(
        &[primary.url.as_str(), backup.url.as_str()],
        ms(300),    // attempt_timeout
        ms(2000),   // total_deadline
        ms(10_000), // cooldown_duration
    );

    let started = Instant::now();
    let result = router.get_latest_slot().await;
    let elapsed = started.elapsed();

    assert_eq!(result, Ok(BACKUP_SLOT), "should get the slot from the backup");
    assert_eq!(primary.hits(), 1, "primary should have been tried once");
    assert_eq!(backup.hits(), 1, "backup should have been tried once");
    assert!(elapsed >= ms(300), "should wait for the primary's attempt timeout");
    assert!(elapsed < ms(2000), "should finish within total_deadline, took {elapsed:?}");
}

#[tokio::test]
async fn skips_failed_primary_while_cooling_down() {
    let primary = MockNode::start(PRIMARY_SLOT, false).await; // hangs
    let backup = MockNode::start(BACKUP_SLOT, true).await;

    let router = RpcRouter::new(
        &[primary.url.as_str(), backup.url.as_str()],
        ms(300),    // attempt_timeout
        ms(2000),   // total_deadline
        ms(10_000), // cooldown_duration: long, so it stays active
    );

    // First request: primary times out and goes on cooldown.
    assert_eq!(router.get_latest_slot().await, Ok(BACKUP_SLOT));
    assert_eq!(primary.hits(), 1);

    // Second request: primary is skipped, so no waiting for a timeout.
    let started = Instant::now();
    let result = router.get_latest_slot().await;
    let elapsed = started.elapsed();

    assert_eq!(result, Ok(BACKUP_SLOT));
    assert_eq!(primary.hits(), 1, "primary should not be contacted during cooldown");
    assert_eq!(backup.hits(), 2);
    assert!(elapsed < ms(300), "second request should skip the primary right away, took {elapsed:?}");
}

#[tokio::test]
async fn returns_to_primary_after_cooldown_expires() {
    let primary = MockNode::start(PRIMARY_SLOT, false).await; // hangs at first
    let backup = MockNode::start(BACKUP_SLOT, true).await;

    let router = RpcRouter::new(
        &[primary.url.as_str(), backup.url.as_str()],
        ms(300),  // attempt_timeout
        ms(2000), // total_deadline
        ms(500),  // cooldown_duration: short, so it expires during the test
    );

    // First request: primary times out, backup answers, primary goes on cooldown.
    assert_eq!(router.get_latest_slot().await, Ok(BACKUP_SLOT));

    // Primary recovers, but its cooldown is still active, so it's skipped.
    primary.set_healthy(true);
    assert_eq!(router.get_latest_slot().await, Ok(BACKUP_SLOT));
    assert_eq!(primary.hits(), 1, "primary should still be skipped");

    // After the cooldown expires, the router goes back to the primary.
    tokio::time::sleep(ms(600)).await;
    assert_eq!(router.get_latest_slot().await, Ok(PRIMARY_SLOT));
    assert_eq!(primary.hits(), 2);
    assert_eq!(backup.hits(), 2, "backup should not be used once primary is back");
}
