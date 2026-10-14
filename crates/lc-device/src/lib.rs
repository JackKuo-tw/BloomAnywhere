//! lc-device: talks to phones.
//!
//! CONTRACT FILE: `lc-app` depends on everything declared in this file.
//! Keep the public signatures stable; put implementations in `ios`, `android`, `mock`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub mod android;
pub mod ios;
pub mod mock;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Ios,
    Android,
    Mock,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DeviceInfo {
    /// iOS UDID, adb serial, or "mock".
    pub id: String,
    pub name: String,
    pub platform: Platform,
    pub os_version: String,
    pub model: String,
    /// "usb" | "network" | "emulator" | "simulated"
    pub connection: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectOptions {
    /// Directory for cached Developer Disk Images / personalization manifests.
    pub cache_dir: PathBuf,
    /// Mount the (personalized) Developer Disk Image automatically when missing.
    pub auto_mount: bool,
    /// Include the mock device in `list_devices`.
    pub enable_mock: bool,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        Self {
            cache_dir: std::env::temp_dir().join("bloomanywhere"),
            auto_mount: true,
            enable_mock: false,
        }
    }
}

/// An open, long-lived location simulation channel to one device.
///
/// NOTE (iOS 17+): the simulated location is reset when the DVT connection closes,
/// so implementations must keep their connection open for the session lifetime.
#[async_trait::async_trait]
pub trait LocationSession: Send {
    fn info(&self) -> &DeviceInfo;
    /// Push a simulated location. Implementations should transparently reconnect once on failure.
    async fn set_location(&mut self, lat: f64, lng: f64) -> anyhow::Result<()>;
    /// Stop simulation and restore the real GPS location.
    async fn clear(&mut self) -> anyhow::Result<()>;
    /// Liveness check (cheap). Default: assume alive.
    async fn ping(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// List devices across all backends. Backend errors (e.g. no usbmuxd, no adb) are logged
/// and skipped, never fatal. The mock device is appended last when `opts.enable_mock`.
pub async fn list_devices(opts: &ConnectOptions) -> anyhow::Result<Vec<DeviceInfo>> {
    let mut out = Vec::new();
    match ios::list().await {
        Ok(mut v) => out.append(&mut v),
        Err(e) => tracing::debug!("ios list failed: {e:#}"),
    }
    match android::list().await {
        Ok(mut v) => out.append(&mut v),
        Err(e) => tracing::debug!("android list failed: {e:#}"),
    }
    if opts.enable_mock {
        out.push(mock::info());
    }
    Ok(out)
}

/// Open a session to the device with the given id (as returned by `list_devices`).
pub async fn connect(id: &str, opts: &ConnectOptions) -> anyhow::Result<Box<dyn LocationSession>> {
    if id == mock::MOCK_ID {
        return Ok(Box::new(mock::MockSession::new()));
    }
    let devices = list_devices(opts).await?;
    let info = devices
        .into_iter()
        .find(|d| d.id == id)
        .ok_or_else(|| anyhow::anyhow!("device {id} not found"))?;
    match info.platform {
        Platform::Ios => Ok(Box::new(ios::IosSession::connect(info, opts).await?)),
        Platform::Android => Ok(Box::new(
            android::AndroidSession::connect(info, opts).await?,
        )),
        Platform::Mock => Ok(Box::new(mock::MockSession::new())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_devices_mock_enabled() {
        let opts = ConnectOptions {
            enable_mock: true,
            ..Default::default()
        };
        let devices = list_devices(&opts).await.unwrap();
        assert!(!devices.is_empty());
        assert!(devices
            .iter()
            .any(|d| d.id == mock::MOCK_ID && d.platform == Platform::Mock));
    }

    #[tokio::test]
    async fn test_list_devices_mock_disabled() {
        let opts = ConnectOptions {
            enable_mock: false,
            ..Default::default()
        };
        let devices = list_devices(&opts).await.unwrap();
        assert!(!devices.iter().any(|d| d.id == mock::MOCK_ID));
    }

    #[tokio::test]
    async fn test_connect_mock_direct() {
        let opts = ConnectOptions::default();
        let mut session = connect(mock::MOCK_ID, &opts).await.unwrap();
        assert_eq!(session.info().platform, Platform::Mock);
        assert_eq!(session.info().id, mock::MOCK_ID);

        // Test set_location
        session.set_location(25.033, 121.565).await.unwrap();
        // Test clear
        session.clear().await.unwrap();
        // Test ping
        session.ping().await.unwrap();
    }

    #[tokio::test]
    async fn test_connect_nonexistent_device() {
        let opts = ConnectOptions::default();
        let res = connect("nonexistent_device_id_999", &opts).await;
        assert!(res.is_err());
    }
}
