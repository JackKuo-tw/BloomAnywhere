//! iOS backend using the pure-Rust `idevice` crate.
//!
//! Connects to iOS devices via usbmuxd (Unix socket `/var/run/usbmuxd` or Windows TCP 27015).
//! - For iOS <= 16: connects to the lockdown service `com.apple.dt.simulatelocation`.
//! - For iOS 17+: uses CoreDeviceProxy (CDTunnel IPv6) + RSD (Remote Service Discovery) + DVT
//!   simulation channel, with clear diagnostic messages regarding Developer Mode.

use crate::{ConnectOptions, DeviceInfo, LocationSession, Platform};
use idevice::{
    provider::RsdProvider,
    services::{
        core_device_proxy::CoreDeviceProxy,
        dvt::{location_simulation::LocationSimulationClient, remote_server::RemoteServerClient},
        lockdown::LockdownClient,
        rsd::RsdHandshake,
        simulate_location::LocationSimulationService,
    },
    usbmuxd::UsbmuxdAddr,
    IdeviceService,
};
use tokio::sync::{mpsc, oneshot};

/// Parses major iOS version number from a version string (e.g. "17.4.1" -> 17).
pub fn parse_major_version(os_version: &str) -> Option<u32> {
    os_version.split('.').next().and_then(|s| s.parse().ok())
}

/// Converts an Apple ProductType (e.g. "iPhone14,2") into a human-readable model name.
pub fn friendly_ios_model(product_type: &str) -> String {
    match product_type {
        "iPhone10,1" | "iPhone10,4" => "iPhone 8".to_string(),
        "iPhone10,2" | "iPhone10,5" => "iPhone 8 Plus".to_string(),
        "iPhone10,3" | "iPhone10,6" => "iPhone X".to_string(),
        "iPhone11,2" => "iPhone XS".to_string(),
        "iPhone11,4" | "iPhone11,6" => "iPhone XS Max".to_string(),
        "iPhone11,8" => "iPhone XR".to_string(),
        "iPhone12,1" => "iPhone 11".to_string(),
        "iPhone12,3" => "iPhone 11 Pro".to_string(),
        "iPhone12,5" => "iPhone 11 Pro Max".to_string(),
        "iPhone12,8" => "iPhone SE (2nd gen)".to_string(),
        "iPhone13,1" => "iPhone 12 mini".to_string(),
        "iPhone13,2" => "iPhone 12".to_string(),
        "iPhone13,3" => "iPhone 12 Pro".to_string(),
        "iPhone13,4" => "iPhone 12 Pro Max".to_string(),
        "iPhone14,2" => "iPhone 13 Pro".to_string(),
        "iPhone14,3" => "iPhone 13 Pro Max".to_string(),
        "iPhone14,4" => "iPhone 13 mini".to_string(),
        "iPhone14,5" => "iPhone 13".to_string(),
        "iPhone14,6" => "iPhone SE (3rd gen)".to_string(),
        "iPhone14,7" => "iPhone 14".to_string(),
        "iPhone14,8" => "iPhone 14 Plus".to_string(),
        "iPhone15,2" => "iPhone 14 Pro".to_string(),
        "iPhone15,3" => "iPhone 14 Pro Max".to_string(),
        "iPhone15,4" => "iPhone 15".to_string(),
        "iPhone15,5" => "iPhone 15 Plus".to_string(),
        "iPhone16,1" => "iPhone 15 Pro".to_string(),
        "iPhone16,2" => "iPhone 15 Pro Max".to_string(),
        "iPhone17,1" => "iPhone 16 Pro".to_string(),
        "iPhone17,2" => "iPhone 16 Pro Max".to_string(),
        "iPhone17,3" => "iPhone 16".to_string(),
        "iPhone17,4" => "iPhone 16 Plus".to_string(),
        other => other.to_string(),
    }
}

/// Builds the 4-byte big endian message type 0 followed by lat/lng strings for iOS <= 16.
pub fn build_simulate_location_message(lat: f64, lng: f64) -> Vec<u8> {
    let lat_str = format!("{lat:.6}");
    let lng_str = format!("{lng:.6}");
    let message_type: [u8; 4] = [0x00, 0x00, 0x00, 0x00];
    let lat_len = lat_str.len() as u32;
    let lng_len = lng_str.len() as u32;
    let mut out = Vec::new();
    out.extend_from_slice(&message_type);
    out.extend_from_slice(&lat_len.to_be_bytes());
    out.extend_from_slice(lat_str.as_bytes());
    out.extend_from_slice(&lng_len.to_be_bytes());
    out.extend_from_slice(lng_str.as_bytes());
    out
}

/// Builds the 4-byte big endian message type 1 for stopping simulation in iOS <= 16.
pub fn build_stop_location_message() -> [u8; 4] {
    [0x00, 0x00, 0x00, 0x01]
}

/// Formats diagnostic instructions for iOS 17+ Developer Mode and tunnel requirements.
pub fn ios17_developer_mode_diagnostic(device_id: &str, underlying_err: &str) -> String {
    format!(
        "iOS 17+ 裝置 ({device_id}) 定位模擬連線失敗。\n\
         【機制說明】：\n\
         iOS 17 起 Apple 停用了傳統的 com.apple.dt.simulatelocation 服務，\n\
         改為採用 CoreDeviceProxy (CDTunnel IPv6 虛擬通道) 搭配 RSD (Remote Service Discovery) 及 DVT 協定。\n\
         此機制必須在手機上啟用「開發者模式」且建立雙向安全配對。\n\
         【排除步驟】：\n\
         1. 開啟開發者模式：請在 iPhone 設定 > 隱私權與安全性 > 開發者模式 開啟並重新開機。\n\
         2. 重開機後解鎖：解鎖 iPhone 螢幕，在彈出視窗中點選「開啟」並輸入鎖定密碼。\n\
         3. 信任電腦：在 iPhone 彈出「信任這部電腦？」時點選「信任」。\n\
         4. 重新連接本軟體。\n\
         底層錯誤資訊: {underlying_err}"
    )
}

/// Formats diagnostic instructions for iOS <= 16 DDI requirements.
pub fn ios16_ddi_diagnostic(device_id: &str, underlying_err: &str) -> String {
    format!(
        "iOS <= 16 裝置 ({device_id}) 定位模擬連線失敗。\n\
         【機制說明】：\n\
         iOS <= 16 使用 lockdown 服務 com.apple.dt.simulatelocation。\n\
         此服務需要裝置掛載 Developer Disk Image (開發者磁碟映像檔，DDI)。\n\
         【排除步驟】：\n\
         1. 確保 iPhone 已解鎖並信任此電腦。\n\
         2. 開啟 Xcode (或使用 ideviceimagemounter) 自動掛載對應 iOS 版本的 Developer Disk Image。\n\
         底層錯誤資訊: {underlying_err}"
    )
}

/// List connected iOS devices via usbmuxd.
///
/// Connects to `/var/run/usbmuxd` (or TCP 27015 on Windows).
/// If usbmuxd is unavailable or returns 0 devices, returns `Ok(vec![])`.
pub async fn list() -> anyhow::Result<Vec<DeviceInfo>> {
    let addr = UsbmuxdAddr::from_env_var().unwrap_or_default();
    let mut mux = match addr.connect(0).await {
        Ok(conn) => conn,
        Err(e) => {
            tracing::debug!("usbmuxd connection failed: {e}");
            return Ok(vec![]);
        }
    };

    let mux_devices = match mux.get_devices().await {
        Ok(devs) => devs,
        Err(e) => {
            tracing::debug!("failed to query devices from usbmuxd: {e}");
            return Ok(vec![]);
        }
    };

    let mut devices = Vec::new();

    for dev in mux_devices {
        let connection = match &dev.connection_type {
            idevice::usbmuxd::Connection::Usb => "usb",
            idevice::usbmuxd::Connection::Network(_) => "network",
            idevice::usbmuxd::Connection::Unknown(_) => "usb",
        }
        .to_string();

        let provider = dev.to_provider(addr.clone(), "lc-device");

        let (name, os_version, model) = match LockdownClient::connect(&provider).await {
            Ok(mut lockdown) => {
                let dev_name = lockdown
                    .get_value(Some("DeviceName"), None)
                    .await
                    .ok()
                    .and_then(|v| v.as_string().map(String::from));

                let prod_version = lockdown
                    .get_value(Some("ProductVersion"), None)
                    .await
                    .ok()
                    .and_then(|v| v.as_string().map(String::from));

                let prod_type = lockdown
                    .get_value(Some("ProductType"), None)
                    .await
                    .ok()
                    .and_then(|v| v.as_string().map(String::from));

                let raw_model = prod_type.unwrap_or_else(|| "iPhone".to_string());
                let friendly_model = friendly_ios_model(&raw_model);
                let name = dev_name.unwrap_or_else(|| friendly_model.clone());
                let os_version = prod_version.unwrap_or_else(|| "-".to_string());

                (name, os_version, friendly_model)
            }
            Err(e) => {
                tracing::debug!("lockdown failed for device {}: {e}", dev.udid);
                ("iPhone".to_string(), "-".to_string(), "iPhone".to_string())
            }
        };

        devices.push(DeviceInfo {
            id: dev.udid,
            name,
            platform: Platform::Ios,
            os_version,
            model,
            connection,
        });
    }

    Ok(devices)
}

enum DvtCommand {
    SetLocation {
        lat: f64,
        lng: f64,
        reply: oneshot::Sender<anyhow::Result<()>>,
    },
    Clear {
        reply: oneshot::Sender<anyhow::Result<()>>,
    },
}

pub struct DvtSession {
    tx: mpsc::Sender<DvtCommand>,
    _worker: tokio::task::JoinHandle<()>,
}

impl DvtSession {
    async fn set_location(&self, lat: f64, lng: f64) -> anyhow::Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(DvtCommand::SetLocation {
                lat,
                lng,
                reply: reply_tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("DVT worker task terminated"))?;
        reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("DVT worker dropped response"))?
    }

    async fn clear(&self) -> anyhow::Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(DvtCommand::Clear { reply: reply_tx })
            .await
            .map_err(|_| anyhow::anyhow!("DVT worker task terminated"))?;
        reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("DVT worker dropped response"))?
    }
}

pub enum IosSessionBackend {
    /// iOS <= 16: standard lockdown com.apple.dt.simulatelocation service
    Lockdown(LocationSimulationService),
    /// iOS 17+: CoreDeviceProxy + RSD + DVT simulation channel
    Dvt(DvtSession),
}

pub struct IosSession {
    info: DeviceInfo,
    backend: IosSessionBackend,
    pub last_location: Option<(f64, f64)>,
}

impl IosSession {
    pub async fn connect(info: DeviceInfo, _opts: &ConnectOptions) -> anyhow::Result<Self> {
        let addr = UsbmuxdAddr::from_env_var().unwrap_or_default();
        let mut mux = addr
            .connect(0)
            .await
            .map_err(|e| anyhow::anyhow!("無法連接 usbmuxd 服務 ({e})"))?;

        let dev = mux
            .get_device(&info.id)
            .await
            .map_err(|e| anyhow::anyhow!("usbmuxd 中未找到裝置 {} ({e})", info.id))?;

        let provider = dev.to_provider(addr, "lc-device");
        let major = parse_major_version(&info.os_version).unwrap_or(16);

        if major >= 17 {
            // iOS 17+: try CoreDeviceProxy + RSD tunnel stack
            match connect_ios17_dvt(&provider).await {
                Ok(dvt_session) => {
                    tracing::info!("iOS 17+ DVT simulation channel connected for {}", info.id);
                    Ok(Self {
                        info,
                        backend: IosSessionBackend::Dvt(dvt_session),
                        last_location: None,
                    })
                }
                Err(e) => {
                    // Try fallback to legacy simulatelocation in case user has custom bridge
                    if let Ok(svc) = LocationSimulationService::connect(&provider).await {
                        tracing::info!(
                            "iOS 17+ connected via legacy simulatelocation for {}",
                            info.id
                        );
                        return Ok(Self {
                            info,
                            backend: IosSessionBackend::Lockdown(svc),
                            last_location: None,
                        });
                    }
                    anyhow::bail!(
                        "{}",
                        ios17_developer_mode_diagnostic(&info.id, &e.to_string())
                    )
                }
            }
        } else {
            // iOS <= 16: use lockdown LocationSimulationService
            match LocationSimulationService::connect(&provider).await {
                Ok(svc) => {
                    tracing::info!("iOS <= 16 simulatelocation connected for {}", info.id);
                    Ok(Self {
                        info,
                        backend: IosSessionBackend::Lockdown(svc),
                        last_location: None,
                    })
                }
                Err(e) => {
                    anyhow::bail!("{}", ios16_ddi_diagnostic(&info.id, &e.to_string()))
                }
            }
        }
    }
}

/// Establishes the iOS 17+ CoreDeviceProxy -> software TUN -> RSD -> DVT location channel.
async fn connect_ios17_dvt(
    provider: &dyn idevice::provider::IdeviceProvider,
) -> anyhow::Result<DvtSession> {
    let proxy = CoreDeviceProxy::connect(provider)
        .await
        .map_err(|e| anyhow::anyhow!("CoreDeviceProxy 連線失敗: {e}"))?;

    let rsd_port = proxy.tunnel_info().server_rsd_port;
    let adapter = proxy
        .create_software_tunnel()
        .map_err(|e| anyhow::anyhow!("建立 CDTunnel userspace TCP stack 失敗: {e}"))?;

    let mut handle = adapter.to_async_handle();

    let rsd_stream = handle
        .connect_to_service_port(rsd_port)
        .await
        .map_err(|e| anyhow::anyhow!("連線至 RSD 連接埠 {rsd_port} 失敗: {e}"))?;

    let handshake = RsdHandshake::new(rsd_stream)
        .await
        .map_err(|e| anyhow::anyhow!("RSD Handshake 失敗: {e}"))?;

    let service = handshake
        .services
        .get("com.apple.instruments.dtservicehub")
        .ok_or_else(|| {
            anyhow::anyhow!("RSD 服務清單中未找到 com.apple.instruments.dtservicehub")
        })?;

    let dt_stream = handle
        .connect_to_service_port(service.port)
        .await
        .map_err(|e| anyhow::anyhow!("連線至 dtservicehub 連接埠 {} 失敗: {e}", service.port))?;

    let mut client = RemoteServerClient::new(dt_stream);

    // Test establishing LocationSimulationClient before launching worker
    let (ready_tx, ready_rx) = oneshot::channel();
    let (tx, mut rx) = mpsc::channel::<DvtCommand>(32);

    let worker = tokio::spawn(async move {
        // Keep handle alive for the lifetime of the tunnel connection
        let _handle = handle;

        let mut sim_client = match LocationSimulationClient::new(&mut client).await {
            Ok(c) => {
                let _ = ready_tx.send(Ok(()));
                c
            }
            Err(e) => {
                let _ = ready_tx.send(Err(anyhow::anyhow!(
                    "建立 LocationSimulationClient 失敗: {e}"
                )));
                return;
            }
        };

        while let Some(cmd) = rx.recv().await {
            match cmd {
                DvtCommand::SetLocation { lat, lng, reply } => {
                    let res = sim_client
                        .set(lat, lng)
                        .await
                        .map_err(|e| anyhow::anyhow!("DVT set_location 失敗: {e}"));
                    let _ = reply.send(res);
                }
                DvtCommand::Clear { reply } => {
                    let res = sim_client
                        .clear()
                        .await
                        .map_err(|e| anyhow::anyhow!("DVT clear 失敗: {e}"));
                    let _ = reply.send(res);
                }
            }
        }
    });

    ready_rx
        .await
        .map_err(|_| anyhow::anyhow!("DVT 初始化任務提前結束"))??;

    Ok(DvtSession {
        tx,
        _worker: worker,
    })
}

#[async_trait::async_trait]
impl LocationSession for IosSession {
    fn info(&self) -> &DeviceInfo {
        &self.info
    }

    async fn set_location(&mut self, lat: f64, lng: f64) -> anyhow::Result<()> {
        match &mut self.backend {
            IosSessionBackend::Lockdown(svc) => {
                let lat_str = format!("{lat:.6}");
                let lng_str = format!("{lng:.6}");
                svc.set(&lat_str, &lng_str)
                    .await
                    .map_err(|e| anyhow::anyhow!("iOS lockdown set_location 失敗: {e}"))?;
            }
            IosSessionBackend::Dvt(dvt) => {
                dvt.set_location(lat, lng).await?;
            }
        }

        self.last_location = Some((lat, lng));
        tracing::debug!(
            "iOS device {} location updated to lat={lat:.6}, lng={lng:.6}",
            self.info.id
        );
        Ok(())
    }

    async fn clear(&mut self) -> anyhow::Result<()> {
        match &mut self.backend {
            IosSessionBackend::Lockdown(svc) => {
                svc.clear()
                    .await
                    .map_err(|e| anyhow::anyhow!("iOS lockdown clear 失敗: {e}"))?;
            }
            IosSessionBackend::Dvt(dvt) => {
                dvt.clear().await?;
            }
        }

        self.last_location = None;
        tracing::debug!("iOS device {} location cleared", self.info.id);
        Ok(())
    }

    async fn ping(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_major_version() {
        assert_eq!(parse_major_version("17.4.1"), Some(17));
        assert_eq!(parse_major_version("16.5"), Some(16));
        assert_eq!(parse_major_version("18.0"), Some(18));
        assert_eq!(parse_major_version("invalid"), None);
        assert_eq!(parse_major_version(""), None);
    }

    #[test]
    fn test_friendly_ios_model() {
        assert_eq!(friendly_ios_model("iPhone14,2"), "iPhone 13 Pro");
        assert_eq!(friendly_ios_model("iPhone15,2"), "iPhone 14 Pro");
        assert_eq!(friendly_ios_model("iPhone16,1"), "iPhone 15 Pro");
        assert_eq!(friendly_ios_model("iPhone17,1"), "iPhone 16 Pro");
        assert_eq!(friendly_ios_model("iPad13,18"), "iPad13,18");
    }

    #[test]
    fn test_build_simulate_location_message() {
        let lat = 25.033;
        let lng = 121.5654;
        let msg = build_simulate_location_message(lat, lng);

        // First 4 bytes: message type 0
        assert_eq!(&msg[0..4], &[0x00, 0x00, 0x00, 0x00]);

        // Next 4 bytes: length of lat string ("25.033000".len() = 9)
        let lat_len = u32::from_be_bytes(msg[4..8].try_into().unwrap());
        assert_eq!(lat_len, 9);
        assert_eq!(&msg[8..8 + 9], b"25.033000");

        // Next 4 bytes: length of lng string ("121.565400".len() = 10)
        let offset = 8 + 9;
        let lng_len = u32::from_be_bytes(msg[offset..offset + 4].try_into().unwrap());
        assert_eq!(lng_len, 10);
        assert_eq!(&msg[offset + 4..offset + 4 + 10], b"121.565400");
    }

    #[test]
    fn test_build_stop_location_message() {
        let msg = build_stop_location_message();
        assert_eq!(msg, [0x00, 0x00, 0x00, 0x01]);
    }

    #[test]
    fn test_ios17_diagnostic_message() {
        let diag = ios17_developer_mode_diagnostic("00008101-001234", "Connection refused");
        assert!(diag.contains("00008101-001234"));
        assert!(diag.contains("請在 iPhone 設定 > 隱私權與安全性 > 開發者模式 開啟並重新開機"));
        assert!(diag.contains("CoreDeviceProxy"));
        assert!(diag.contains("RSD"));
    }

    #[test]
    fn test_ios16_diagnostic_message() {
        let diag = ios16_ddi_diagnostic("00008030-001234", "ServiceNotFound");
        assert!(diag.contains("00008030-001234"));
        assert!(diag.contains("com.apple.dt.simulatelocation"));
        assert!(diag.contains("Developer Disk Image"));
    }

    #[tokio::test]
    async fn test_ios_list_devices_empty_or_ok() {
        // Since usbmuxd may be running on macOS but without devices plugged in,
        // list() should return Ok(vec![]) without panicking or erroring.
        let result = list().await;
        assert!(result.is_ok());
    }
}
