//! Android backend (adb based).
//!
//! Communicates with Android devices using the `adb` CLI tool.
//! - For emulators (serial starting with `emulator-`): supports direct location simulation
//!   via `adb -s <serial> emu geo fix <lng> <lat>`.
//! - For physical Android devices: returns clear diagnostic instructions explaining that
//!   physical devices require mock location provider settings (e.g. Developer Options ->
//!   Select mock location app) or an assistant APK.

use crate::{ConnectOptions, DeviceInfo, LocationSession, Platform};
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Locate the `adb` binary.
///
/// Precedence:
/// 1. `LC_ADB` environment variable
/// 2. `$ANDROID_HOME/platform-tools/adb`
/// 3. `$ANDROID_SDK_ROOT/platform-tools/adb`
/// 4. `PATH` lookup
pub fn locate_adb() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("LC_ADB") {
        let path = PathBuf::from(p);
        if path.is_file() || which_in_path(&path).is_some() {
            return Some(path);
        }
    }

    if let Ok(home) = std::env::var("ANDROID_HOME") {
        let candidate = PathBuf::from(home)
            .join("platform-tools")
            .join(adb_executable_name());
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    if let Ok(root) = std::env::var("ANDROID_SDK_ROOT") {
        let candidate = PathBuf::from(root)
            .join("platform-tools")
            .join(adb_executable_name());
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    // Check system PATH
    which_in_path(Path::new(adb_executable_name()))
}

#[cfg(windows)]
fn adb_executable_name() -> &'static str {
    "adb.exe"
}

#[cfg(not(windows))]
fn adb_executable_name() -> &'static str {
    "adb"
}

fn which_in_path(bin: &Path) -> Option<PathBuf> {
    if bin.is_absolute() && bin.is_file() {
        return Some(bin.to_path_buf());
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let full = dir.join(bin);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

/// Raw parsed entry from `adb devices -l`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbDeviceEntry {
    pub serial: String,
    pub state: String,
    pub product: Option<String>,
    pub model: Option<String>,
    pub device: Option<String>,
    pub transport_id: Option<String>,
    pub connection: String,
}

/// Parses the standard text output from `adb devices -l`.
pub fn parse_adb_devices_output(output: &str) -> Vec<AdbDeviceEntry> {
    let mut entries = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        // Skip empty lines, headers, and daemon status logs
        if line.is_empty()
            || line.starts_with("List of devices attached")
            || line.starts_with('*')
            || line.starts_with("adb server")
        {
            continue;
        }

        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() < 2 {
            continue;
        }

        let serial = tokens[0].to_string();
        let state = tokens[1].to_string();

        let mut product = None;
        let mut model = None;
        let mut device = None;
        let mut transport_id = None;
        let mut has_usb = false;

        for &tok in &tokens[2..] {
            if let Some((k, v)) = tok.split_once(':') {
                match k {
                    "product" => product = Some(v.to_string()),
                    "model" => model = Some(v.to_string()),
                    "device" => device = Some(v.to_string()),
                    "transport_id" => transport_id = Some(v.to_string()),
                    "usb" => has_usb = true,
                    _ => {}
                }
            }
        }

        let connection = determine_connection_type(&serial, has_usb);

        entries.push(AdbDeviceEntry {
            serial,
            state,
            product,
            model,
            device,
            transport_id,
            connection,
        });
    }

    entries
}

/// Determines whether the connection is emulator, network, or usb.
pub fn determine_connection_type(serial: &str, _has_usb: bool) -> String {
    if serial.starts_with("emulator-") {
        "emulator".to_string()
    } else if serial.contains(':') {
        "network".to_string()
    } else {
        "usb".to_string()
    }
}

/// Builds the `adb emu geo fix <lng> <lat>` command argument list.
///
/// NOTE: Android emulator console accepts longitude first, latitude second!
pub fn build_emu_geo_fix_args(serial: &str, lat: f64, lng: f64) -> Vec<String> {
    vec![
        "-s".to_string(),
        serial.to_string(),
        "emu".to_string(),
        "geo".to_string(),
        "fix".to_string(),
        format!("{lng:.7}"),
        format!("{lat:.7}"),
    ]
}

/// Queries `getprop` on an Android device via adb.
async fn query_getprop(adb: &Path, serial: &str, prop: &str) -> Option<String> {
    let out = Command::new(adb)
        .args(["-s", serial, "shell", "getprop", prop])
        .output()
        .await
        .ok()?;

    if out.status.success() {
        let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !val.is_empty() {
            return Some(val);
        }
    }
    None
}

/// List connected Android devices via `adb devices -l`.
pub async fn list() -> anyhow::Result<Vec<DeviceInfo>> {
    let adb_path = match locate_adb() {
        Some(p) => p,
        None => {
            tracing::debug!(
                "adb not found in PATH or ANDROID_HOME; skipping Android device listing"
            );
            return Ok(vec![]);
        }
    };

    let output = match Command::new(&adb_path)
        .args(["devices", "-l"])
        .output()
        .await
    {
        Ok(out) => out,
        Err(e) => {
            tracing::debug!("failed to execute adb devices -l: {e}");
            return Ok(vec![]);
        }
    };

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        tracing::debug!("adb devices -l exited with status {}: {err}", output.status);
        return Ok(vec![]);
    }

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let entries = parse_adb_devices_output(&stdout_str);

    let mut devices = Vec::new();
    for entry in entries {
        // Only devices in "device" state are ready for commands
        if entry.state != "device" {
            tracing::debug!(
                "skipping Android device {} with status '{}'",
                entry.serial,
                entry.state
            );
            continue;
        }

        let serial = entry.serial;
        let os_version = query_getprop(&adb_path, &serial, "ro.build.version.release")
            .await
            .unwrap_or_else(|| "-".to_string());

        let model = query_getprop(&adb_path, &serial, "ro.product.model")
            .await
            .or_else(|| entry.model.map(|m| m.replace('_', " ")))
            .unwrap_or_else(|| "Android Device".to_string());

        let name = if model.is_empty() || model == "Android Device" {
            format!("Android ({serial})")
        } else {
            model.clone()
        };

        devices.push(DeviceInfo {
            id: serial,
            name,
            platform: Platform::Android,
            os_version,
            model,
            connection: entry.connection,
        });
    }

    Ok(devices)
}

pub struct AndroidSession {
    info: DeviceInfo,
    adb_path: PathBuf,
    pub last_location: Option<(f64, f64)>,
}

impl AndroidSession {
    pub async fn connect(info: DeviceInfo, _opts: &ConnectOptions) -> anyhow::Result<Self> {
        let is_emulator = info.connection == "emulator" || info.id.starts_with("emulator-");

        if !is_emulator {
            anyhow::bail!(
                "實體 Android 裝置 ({}) 尚未支援直接定位模擬。\n\
                 【原因說明】：\n\
                 Android 系統為安全考量，不允許透過一般 ADB 指令全域修改 GPS 定位。\n\
                 【解決方式】：\n\
                 1. 改用 Android 模擬器 (Android Emulator)：無需額外設定，支援直接透過 adb emu geo fix 模擬定位。\n\
                 2. 實體裝置如需模擬，需在手機開啟「開發人員選項」->「選取模擬位置應用程式」，並安裝輔助 APK（如 GpsAssistant）。\n\
                 3. 或改用 iOS 裝置 / 虛擬模擬裝置 (Mock)。",
                info.id
            );
        }

        let adb_path = locate_adb().ok_or_else(|| {
            anyhow::anyhow!(
                "未找到 adb 執行檔，請確認 Android SDK 已安裝並設定至 PATH 或 ANDROID_HOME"
            )
        })?;

        // Verify emulator responsiveness via `adb -s <serial> get-state`
        let status = Command::new(&adb_path)
            .args(["-s", &info.id, "get-state"])
            .output()
            .await;

        match status {
            Ok(out) if out.status.success() => {
                let state = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if state != "device" {
                    anyhow::bail!("Android 模擬器 {} 狀態異常: {}", info.id, state);
                }
            }
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                anyhow::bail!("無法連線至 Android 模擬器 {}: {}", info.id, err);
            }
            Err(e) => {
                anyhow::bail!("執行 adb 連線檢查失敗: {}", e);
            }
        }

        Ok(Self {
            info,
            adb_path,
            last_location: None,
        })
    }
}

#[async_trait::async_trait]
impl LocationSession for AndroidSession {
    fn info(&self) -> &DeviceInfo {
        &self.info
    }

    async fn set_location(&mut self, lat: f64, lng: f64) -> anyhow::Result<()> {
        let args = build_emu_geo_fix_args(&self.info.id, lat, lng);
        let out = Command::new(&self.adb_path)
            .args(&args)
            .output()
            .await
            .map_err(|e| anyhow::anyhow!("執行 adb emu geo fix 失敗: {e}"))?;

        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let msg = if err.is_empty() {
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            } else {
                err
            };
            anyhow::bail!("模擬器設定位置失敗 ({msg})");
        }

        self.last_location = Some((lat, lng));
        tracing::debug!(
            "Android emulator {} location updated to lat={lat:.7}, lng={lng:.7}",
            self.info.id
        );
        Ok(())
    }

    async fn clear(&mut self) -> anyhow::Result<()> {
        self.last_location = None;
        tracing::debug!(
            "Android emulator {} location simulation cleared",
            self.info.id
        );
        Ok(())
    }

    async fn ping(&mut self) -> anyhow::Result<()> {
        let out = Command::new(&self.adb_path)
            .args(["-s", &self.info.id, "get-state"])
            .output()
            .await
            .map_err(|e| anyhow::anyhow!("adb ping failed: {e}"))?;

        if out.status.success() {
            Ok(())
        } else {
            anyhow::bail!("Android emulator {} unreachable", self.info.id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_adb_devices_empty() {
        let output = "";
        let entries = parse_adb_devices_output(output);
        assert!(entries.is_empty());

        let output2 = "List of devices attached\n\n";
        let entries2 = parse_adb_devices_output(output2);
        assert!(entries2.is_empty());
    }

    #[test]
    fn test_parse_adb_devices_daemon_banner() {
        let output = r#"* daemon not running; starting now at tcp:5037
* daemon started successfully
List of devices attached
emulator-5554          device product:sdk_gphone64_arm64 model:sdk_gphone64_arm64 device:emu64a transport_id:1
"#;
        let entries = parse_adb_devices_output(output);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].serial, "emulator-5554");
        assert_eq!(entries[0].state, "device");
        assert_eq!(entries[0].connection, "emulator");
        assert_eq!(entries[0].model.as_deref(), Some("sdk_gphone64_arm64"));
    }

    #[test]
    fn test_parse_adb_devices_mixed() {
        let output = r#"List of devices attached
emulator-5554          device product:sdk_gphone64_arm64 model:sdk_gphone64_arm64 device:emu64a transport_id:1
0123456789ABCDEF       device usb:337641472X product:taimen model:Pixel_2_XL device:taimen transport_id:2
192.168.1.100:5555     device product:coral model:Pixel_4 device:coral transport_id:3
unauth_serial          unauthorized usb:123
offline_serial         offline
"#;
        let entries = parse_adb_devices_output(output);
        assert_eq!(entries.len(), 5);

        // Emulator
        assert_eq!(entries[0].serial, "emulator-5554");
        assert_eq!(entries[0].connection, "emulator");
        assert_eq!(entries[0].state, "device");

        // USB physical device
        assert_eq!(entries[1].serial, "0123456789ABCDEF");
        assert_eq!(entries[1].connection, "usb");
        assert_eq!(entries[1].state, "device");
        assert_eq!(entries[1].model.as_deref(), Some("Pixel_2_XL"));

        // Network device
        assert_eq!(entries[2].serial, "192.168.1.100:5555");
        assert_eq!(entries[2].connection, "network");
        assert_eq!(entries[2].state, "device");

        // Unauthorized
        assert_eq!(entries[3].serial, "unauth_serial");
        assert_eq!(entries[3].state, "unauthorized");

        // Offline
        assert_eq!(entries[4].serial, "offline_serial");
        assert_eq!(entries[4].state, "offline");
    }

    #[test]
    fn test_parse_adb_devices_crlf() {
        let output = "List of devices attached\r\nemulator-5556\tdevice product:sdk model:Emu device:emu\r\n";
        let entries = parse_adb_devices_output(output);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].serial, "emulator-5556");
        assert_eq!(entries[0].connection, "emulator");
    }

    #[test]
    fn test_build_emu_geo_fix_args() {
        let lat = 25.0330;
        let lng = 121.5654;
        let args = build_emu_geo_fix_args("emulator-5554", lat, lng);

        assert_eq!(args[0], "-s");
        assert_eq!(args[1], "emulator-5554");
        assert_eq!(args[2], "emu");
        assert_eq!(args[3], "geo");
        assert_eq!(args[4], "fix");
        // Verify longitude comes first, latitude second!
        assert_eq!(args[5], "121.5654000");
        assert_eq!(args[6], "25.0330000");
    }

    #[tokio::test]
    async fn test_physical_device_connect_error() {
        let info = DeviceInfo {
            id: "0123456789ABCDEF".to_string(),
            name: "Pixel 7".to_string(),
            platform: Platform::Android,
            os_version: "14".to_string(),
            model: "Pixel 7".to_string(),
            connection: "usb".to_string(),
        };
        let opts = ConnectOptions::default();
        let res = AndroidSession::connect(info, &opts).await;
        assert!(res.is_err());
        let err = res.err().unwrap().to_string();
        assert!(err.contains("實體 Android 裝置"));
        assert!(err.contains("開發人員選項"));
        assert!(err.contains("Android 模擬器"));
    }
}
