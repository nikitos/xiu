pub mod http;

use async_trait::async_trait;
use crate::define::{StreamHubEventMessage};
use crate::define::{HookResponse};

#[async_trait]
pub trait Notifier: Sync + Send {
    async fn on_connect_notify(&self, event: &StreamHubEventMessage) -> Option<HookResponse>;
    async fn on_publish_notify(&self, event: &StreamHubEventMessage);
    async fn on_unpublish_notify(&self, event: &StreamHubEventMessage);
    async fn on_play_notify(&self, event: &StreamHubEventMessage);
    async fn on_stop_notify(&self, event: &StreamHubEventMessage);
    async fn on_hls_notify(&self, event: &StreamHubEventMessage);
    async fn kick_off_client(&self, event: &StreamHubEventMessage);
}
