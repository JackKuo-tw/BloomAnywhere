//! In-memory mock device for demos and tests.
use crate::{DeviceInfo, LocationSession, Platform};

pub const MOCK_ID: &str = "mock";

pub fn info() -> DeviceInfo {
    DeviceInfo {
        id: MOCK_ID.into(),
        name: "模擬裝置 (Mock)".into(),
        platform: Platform::Mock,
        os_version: "-".into(),
        model: "Virtual".into(),
        connection: "simulated".into(),
    }
}

pub struct MockSession {
    info: DeviceInfo,
    pub last: Option<(f64, f64)>,
    pub pushes: u64,
}

impl MockSession {
    pub fn new() -> Self {
        Self {
            info: info(),
            last: None,
            pushes: 0,
        }
    }
}

impl Default for MockSession {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl LocationSession for MockSession {
    fn info(&self) -> &DeviceInfo {
        &self.info
    }
    async fn set_location(&mut self, lat: f64, lng: f64) -> anyhow::Result<()> {
        self.last = Some((lat, lng));
        self.pushes += 1;
        tracing::debug!("mock set_location {lat:.6},{lng:.6}");
        Ok(())
    }
    async fn clear(&mut self) -> anyhow::Result<()> {
        self.last = None;
        Ok(())
    }
}
