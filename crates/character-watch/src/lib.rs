//! System event observers for MVP-1.
//!
//! The current observer reads only NetworkManager's connectivity state. It does
//! not inspect connection names, notification contents, screen contents, or input.

use character_types::NetworkStatus;
use futures_util::StreamExt;
use tokio::sync::mpsc::Sender;
use zbus::{Connection, proxy};

const NM_STATE_DISCONNECTED: u32 = 20;
const NM_STATE_CONNECTED_LOCAL: u32 = 50;
const NM_STATE_CONNECTED_SITE: u32 = 60;
const NM_STATE_CONNECTED_GLOBAL: u32 = 70;

#[proxy(
    interface = "org.freedesktop.NetworkManager",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager"
)]
trait NetworkManager {
    #[zbus(property)]
    fn state(&self) -> zbus::Result<u32>;

    #[zbus(signal, name = "StateChanged")]
    fn network_state_changed(&self, state: u32) -> zbus::Result<()>;
}

/// Observes NetworkManager state changes and sends only stable connected or
/// disconnected states. The first state initializes consumers without a reaction.
pub async fn watch_network_status(sender: Sender<NetworkStatus>) -> zbus::Result<()> {
    let connection = Connection::system().await?;
    let proxy = NetworkManagerProxy::new(&connection).await?;
    let mut changes = proxy.receive_network_state_changed().await?;

    if let Some(status) = status_from_nm_state(proxy.state().await?)
        && sender.send(status).await.is_err()
    {
        return Ok(());
    }

    while let Some(change) = changes.next().await {
        let state = change.args()?.state;
        if let Some(status) = status_from_nm_state(state)
            && sender.send(status).await.is_err()
        {
            return Ok(());
        }
    }

    Err(zbus::Error::Failure(
        "NetworkManager StateChanged stream ended".to_string(),
    ))
}

fn status_from_nm_state(state: u32) -> Option<NetworkStatus> {
    match state {
        NM_STATE_DISCONNECTED => Some(NetworkStatus::Disconnected),
        NM_STATE_CONNECTED_LOCAL | NM_STATE_CONNECTED_SITE | NM_STATE_CONNECTED_GLOBAL => {
            Some(NetworkStatus::Connected)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_only_stable_networkmanager_states() {
        assert_eq!(
            status_from_nm_state(NM_STATE_DISCONNECTED),
            Some(NetworkStatus::Disconnected)
        );
        for state in [
            NM_STATE_CONNECTED_LOCAL,
            NM_STATE_CONNECTED_SITE,
            NM_STATE_CONNECTED_GLOBAL,
        ] {
            assert_eq!(status_from_nm_state(state), Some(NetworkStatus::Connected));
        }
        for state in [0, 10, 30, 40, 55, 80] {
            assert_eq!(status_from_nm_state(state), None);
        }
    }
}
