use streamhub::define::StreamHubEventSender;
use std::sync::Arc;

use super::session::server_session;
use commonlib::auth::Auth;
use std::net::SocketAddr;
use tokio::io::Error;
use tokio::net::TcpListener;

use streamhub::notify::Notifier;
use config::RtmpConfig;
use std::{
    collections::HashMap, 
    sync::Mutex as StdMutex, 
};

pub struct RtmpServer {
    address: String,
    event_producer: StreamHubEventSender,
    gop_num: usize,
    auth: Option<Auth>,
    rate_limit_config: Option<RtmpConfig>,
    rate_limiter: Arc<StdMutex<HashMap<String, Vec<u64>>>>,
    max_publish_per_stream: u32,
    time_window_seconds: u64,
}

impl RtmpServer {
    pub fn new(
        address: String,
        event_producer: StreamHubEventSender,
        gop_num: usize,
        auth: Option<Auth>,
        rate_limit_config: Option<RtmpConfig>,
    ) -> Self {
        // Extract rate limiting configuration
        let (max_publish_per_stream, time_window_seconds) = if let Some(config) = &rate_limit_config {
            if let Some(rate_limit) = &config.rate_limit {
                if rate_limit.enabled {
                    (
                        rate_limit.max_publish_per_stream.unwrap_or(1),
                        rate_limit.time_window_seconds.unwrap_or(60),
                    )
                } else {
                    (1, 60) // Default values when rate limiting is disabled
                }
            } else {
                (1, 60) // Default values when no rate limit config
            }
        } else {
            (1, 60) // Default values when no config
        };

        log::info!("RTMP Server rate limiter initialized: max_publish_per_stream={}, time_window_seconds={}", 
                   max_publish_per_stream, time_window_seconds);

        Self {
            address,
            event_producer,
            gop_num,
            auth,
            rate_limit_config,
            rate_limiter: Arc::new(StdMutex::new(HashMap::new())),
            max_publish_per_stream,
            time_window_seconds,
        }
    }

    pub async fn run(&mut self, notifier: Option<Arc<dyn Notifier>>) -> Result<(), Error> {
        let socket_addr: &SocketAddr = &self.address.parse().unwrap();
        let listener = TcpListener::bind(socket_addr).await?;

        log::info!("Rtmp server listening on tcp://{}", socket_addr);
        loop {
            let (tcp_stream, _) = listener.accept().await?;
            //tcp_stream.set_keepalive(Some(Duration::from_secs(30)))?;

            let mut session = server_session::ServerSession::new(
                tcp_stream,
                self.event_producer.clone(),
                self.gop_num,
                self.auth.clone(),
                notifier.clone(),
                self.rate_limit_config.clone(),
                self.rate_limiter.clone(),
                self.max_publish_per_stream,
                self.time_window_seconds,
            );
            tokio::spawn(async move {
                if let Err(err) = session.run().await {
                    log::info!(
                        "session run error: session_type: {}, app_name: {}, stream_name: {}, err: {}",
                        session.common.session_type,
                        session.app_name,
                        session.stream_name,
                        err
                    );
                }
            });
        }
    }
}
