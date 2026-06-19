use std::sync::Arc;

use tokio::time::{interval, Duration};

use crate::sessions::Sessions;

pub fn spawn_expiry_loop(sessions: Arc<Sessions>, sweep_interval_seconds: u64) {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(sweep_interval_seconds.max(1)));
        loop {
            ticker.tick().await;
            sessions.expire_sessions().await;
        }
    });
}
