//! Game profiles. Bloom mode is the primary/default profile.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SpeedPreset {
    pub id: String,       // e.g. "walk"
    pub label: String,    // zh-TW label, e.g. "散步"
    pub label_en: String, // e.g. "Walk"
    pub kmh: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GameProfile {
    pub id: String, // "bloom", "generic"
    pub name: String,
    pub speed_presets: Vec<SpeedPreset>,
    pub default_speed_kmh: f64,
    /// Above this speed the game stops counting (UI shows a warning).
    pub max_safe_speed_kmh: f64,
    /// Hard upper limit for the speed slider.
    pub max_speed_kmh: f64,
    /// Random lateral GPS jitter amplitude (metres) when realistic mode is on.
    pub jitter_m: f64,
    /// Relative speed variance (0.15 = ±15 %) in realistic mode.
    pub speed_variance: f64,
    /// Stationary natural micro-drift radius in metres (e.g. 0.8m) when idle or paused.
    /// Emulates real atmospheric GPS wander to prevent zero-variance detection.
    pub stationary_drift_m: f64,
    /// Interval between location pushes to the device.
    pub update_interval_ms: u64,
    pub cooldown_enabled: bool,
    /// Short zh-TW tips shown in the UI.
    pub tips: Vec<String>,
}

/// All built-in profiles; the first element is `bloom()`.
pub fn profiles() -> Vec<GameProfile> {
    vec![bloom(), generic()]
}

pub fn bloom() -> GameProfile {
    GameProfile {
        id: "bloom".to_string(),
        name: "Bloom (種花模式)".to_string(),
        speed_presets: vec![
            SpeedPreset {
                id: "walk".to_string(),
                label: "散步".to_string(),
                label_en: "Walk".to_string(),
                kmh: 4.5,
            },
            SpeedPreset {
                id: "brisk_walk".to_string(),
                label: "快走".to_string(),
                label_en: "Brisk walk".to_string(),
                kmh: 6.5,
            },
            SpeedPreset {
                id: "jog".to_string(),
                label: "慢跑".to_string(),
                label_en: "Jog".to_string(),
                kmh: 12.0,
            },
            SpeedPreset {
                id: "bike".to_string(),
                label: "單車/速跑".to_string(),
                label_en: "Bike/Run".to_string(),
                kmh: 18.0,
            },
            SpeedPreset {
                id: "mushroom_scout".to_string(),
                label: "🍄 找大蘑菇".to_string(),
                label_en: "Mushroom Scout".to_string(),
                kmh: 120.0,
            },
        ],
        default_speed_kmh: 5.5,
        max_safe_speed_kmh: 20.0,
        max_speed_kmh: 120.0,
        jitter_m: 1.5,
        speed_variance: 0.15,
        stationary_drift_m: 0.8,
        update_interval_ms: 1000,
        cooldown_enabled: true,
        tips: vec![
            "種花時速上限為 20.0 km/h（最佳效率約 4.0 - 15.0 km/h）。超過 20 km/h 將停止種花並觸發警告！".to_string(),
            "尋找大蘑菇可使用高速巡邏模式 (120 km/h) 快速移動探查全城地圖，抵達蘑菇位置後再進行戰鬥！".to_string(),
            "步數主要依賴手機內建動態感應器 (Apple 健康 / Google Fit)，模擬 GPS 位移不會直接增加步數，但可以觸發種花、大花盛開、以及尋找香菇與探險！".to_string(),
            "同一地點在 5 分鐘內無法重複種花，推薦使用「循環路線」或「大花周邊巡迴 (半徑 > 100m)」繞圈種花。".to_string(),
            "長距離傳送後請遵守冷卻時間 (Cooldown)，避免被遊戲判定位置異常。".to_string(),
            "iOS 16+ 需在手機「設定 > 隱私權與安全性」中開啟「開發者模式」並重新啟動。".to_string(),
        ],
    }
}

pub fn generic() -> GameProfile {
    GameProfile {
        id: "generic".to_string(),
        name: "通用 / Generic".to_string(),
        speed_presets: vec![
            SpeedPreset {
                id: "walk".to_string(),
                label: "步行".to_string(),
                label_en: "Walk".to_string(),
                kmh: 5.0,
            },
            SpeedPreset {
                id: "bike".to_string(),
                label: "自行車".to_string(),
                label_en: "Bike".to_string(),
                kmh: 15.0,
            },
            SpeedPreset {
                id: "car".to_string(),
                label: "汽車".to_string(),
                label_en: "Car".to_string(),
                kmh: 60.0,
            },
        ],
        default_speed_kmh: 15.0,
        max_safe_speed_kmh: 120.0,
        max_speed_kmh: 150.0,
        jitter_m: 0.0,
        speed_variance: 0.0,
        stationary_drift_m: 0.0,
        update_interval_ms: 1000,
        cooldown_enabled: false,
        tips: vec![
            "適用於一般地圖標記、外送、社群打卡等無需遊戲冷卻限制的應用場景。".to_string(),
            "可自訂任意速度，並支援 GPX 軌跡匯入與循環移動。".to_string(),
        ],
    }
}

pub fn by_id(id: &str) -> Option<GameProfile> {
    profiles().into_iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profiles_list() {
        let list = profiles();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "bloom");
        assert_eq!(list[1].id, "generic");
    }

    #[test]
    fn test_bloom_defaults() {
        let p = bloom();
        assert_eq!(p.id, "bloom");
        assert_eq!(p.default_speed_kmh, 5.5);
        assert_eq!(p.max_safe_speed_kmh, 20.0);
        assert_eq!(p.max_speed_kmh, 120.0);
        assert_eq!(p.jitter_m, 1.5);
        assert_eq!(p.speed_variance, 0.15);
        assert_eq!(p.stationary_drift_m, 0.8);
        assert!(p.cooldown_enabled);
        assert_eq!(p.speed_presets.len(), 5);
        assert_eq!(p.tips.len(), 6);
    }

    #[test]
    fn test_generic_defaults() {
        let p = generic();
        assert_eq!(p.id, "generic");
        assert_eq!(p.jitter_m, 0.0);
        assert_eq!(p.speed_variance, 0.0);
        assert_eq!(p.stationary_drift_m, 0.0);
        assert!(!p.cooldown_enabled);
    }

    #[test]
    fn test_by_id() {
        assert!(by_id("bloom").is_some());
        assert!(by_id("generic").is_some());
        assert!(by_id("nonexistent").is_none());
    }
}
