use axum::{
    extract::State,
    response::sse::{Event as SseEvent, KeepAlive, Sse},
    routing::get,
    Router,
};
use futures_util::stream::{self, Stream};
use serde::Serialize;
use std::convert::Infallible;
use tokio::sync::broadcast;

pub const REQUIREMENT_CHANGED: &str = "requirement.changed";
const BROWSER_EVENT_CAPACITY: usize = 256;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BrowserNotification {
    pub category: String,
    pub requirement_id: String,
}

impl BrowserNotification {
    pub fn requirement_changed(requirement_id: impl Into<String>) -> Self {
        Self {
            category: REQUIREMENT_CHANGED.to_owned(),
            requirement_id: requirement_id.into(),
        }
    }
}

#[derive(Clone)]
pub struct BrowserEventHub {
    sender: broadcast::Sender<BrowserNotification>,
}

impl BrowserEventHub {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(BROWSER_EVENT_CAPACITY);
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<BrowserNotification> {
        self.sender.subscribe()
    }

    pub fn publish(&self, notification: BrowserNotification) {
        let _ = self.sender.send(notification);
    }

    pub fn requirement_changed(&self, requirement_id: impl Into<String>) {
        self.publish(BrowserNotification::requirement_changed(requirement_id));
    }
}

impl Default for BrowserEventHub {
    fn default() -> Self {
        Self::new()
    }
}

/// Protected browser notification stream. It is intentionally in-memory and
/// non-durable; HTTP remains the source of canonical Requirement state. If a
/// subscriber misses broadcast notifications, this stream terminates so native
/// EventSource reconnect/refetch can restore canonical state.
pub fn router() -> Router<crate::auth::AuthState> {
    Router::new().route("/events", get(events))
}

fn notification_stream(
    receiver: broadcast::Receiver<BrowserNotification>,
) -> impl Stream<Item = Result<SseEvent, Infallible>> {
    stream::unfold(receiver, |mut receiver| async move {
        loop {
            match receiver.recv().await {
                Ok(notification) => {
                    let Ok(event) = SseEvent::default()
                        .event(notification.category.clone())
                        .json_data(&notification)
                    else {
                        continue;
                    };
                    return Some((Ok(event), receiver));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => return None,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    })
}

async fn events(
    State(state): State<crate::auth::AuthState>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    Sse::new(notification_stream(state.events().subscribe())).keep_alive(KeepAlive::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use serde_json::json;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn publishes_identity_only_requirement_notifications() {
        let hub = BrowserEventHub::new();
        let mut receiver = hub.subscribe();
        hub.requirement_changed("requirement-1");

        let notification = receiver.recv().await.expect("notification");
        assert_eq!(
            serde_json::to_value(notification).expect("serializable notification"),
            json!({
                "category": "requirement.changed",
                "requirement_id": "requirement-1"
            })
        );
    }

    #[tokio::test]
    async fn lagged_subscriber_terminates_stream_for_reconnect() {
        let hub = BrowserEventHub::new();
        let receiver = hub.subscribe();

        for index in 0..=BROWSER_EVENT_CAPACITY {
            hub.requirement_changed(format!("requirement-{index}"));
        }

        let mut stream = Box::pin(notification_stream(receiver));
        let item = timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("lagged stream should terminate promptly");
        assert!(
            item.is_none(),
            "lagged stream must terminate, not skip and continue"
        );
    }

    #[tokio::test]
    async fn route_exposes_sse_response() -> Result<(), Box<dyn std::error::Error>> {
        use axum::{body::Body, http::Request};
        use north_persistence::{AuthStore, PoolOptions};
        use tower::ServiceExt;

        let pool = PoolOptions::new().connect_lazy("postgres://localhost/north")?;
        let state = crate::auth::AuthState::with_log_delivery(AuthStore::new(pool));
        let response = router()
            .with_state(state)
            .oneshot(Request::builder().uri("/events").body(Body::empty())?)
            .await?;

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/event-stream")
        );
        Ok(())
    }

    #[tokio::test]
    async fn auth_router_rejects_events_without_session_cookie(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use axum::{body::Body, http::Request};
        use north_persistence::{AuthStore, PoolOptions};
        use tower::ServiceExt;

        let pool = PoolOptions::new().connect_lazy("postgres://localhost/north")?;
        let state = crate::auth::AuthState::with_log_delivery(AuthStore::new(pool));
        let response = crate::auth::router(state)
            .oneshot(Request::builder().uri("/events").body(Body::empty())?)
            .await?;

        assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
        Ok(())
    }
}
