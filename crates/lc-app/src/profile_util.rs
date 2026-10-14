use lc_core::profile::{GameProfile, SpeedPreset};

pub fn default_bloom() -> GameProfile {
    GameProfile {
        id: "bloom".into(),
        name: "Bloom (種花模式)".into(),
        speed_presets: vec![
            SpeedPreset {
                id: "walk".into(),
                label: "散步".into(),
                label_en: "Walk".into(),
                kmh: 4.5,
            },
            SpeedPreset {
                id: "brisk_walk".into(),
                label: "快走".into(),
                label_en: "Brisk Walk".into(),
                kmh: 6.5,
            },
            SpeedPreset {
                id: "jog".into(),
                label: "慢跑".into(),
                label_en: "Jog".into(),
                kmh: 12.0,
            },
            SpeedPreset {
                id: "bike".into(),
                label: "單車/速跑".into(),
                label_en: "Bike/Run".into(),
                kmh: 18.0,
            },
            SpeedPreset {
                id: "mushroom_scout".into(),
                label: "🍄 找大蘑菇".into(),
                label_en: "Mushroom Scout".into(),
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
            "種花時速請勿超過 20 km/h，否則花朵將停止生長！".into(),
            "尋找大蘑菇可切換為「🍄 找大蘑菇 (120 km/h)」高速巡邏模式快速巡視全城！".into(),
            "建議使用「繞圈種花」功能在公園或大朵花周圍自動走動。".into(),
            "每次傳送距離超過 1 公里時，請注意冷卻時間。".into(),
        ],
    }
}

pub fn default_generic() -> GameProfile {
    GameProfile {
        id: "generic".into(),
        name: "通用模擬 (Generic)".into(),
        speed_presets: vec![
            SpeedPreset {
                id: "slow".into(),
                label: "慢速".into(),
                label_en: "Slow".into(),
                kmh: 5.0,
            },
            SpeedPreset {
                id: "medium".into(),
                label: "中速".into(),
                label_en: "Medium".into(),
                kmh: 15.0,
            },
            SpeedPreset {
                id: "fast".into(),
                label: "快速".into(),
                label_en: "Fast".into(),
                kmh: 30.0,
            },
            SpeedPreset {
                id: "express".into(),
                label: "高速".into(),
                label_en: "Express".into(),
                kmh: 60.0,
            },
        ],
        default_speed_kmh: 15.0,
        max_safe_speed_kmh: 60.0,
        max_speed_kmh: 120.0,
        jitter_m: 0.5,
        speed_variance: 0.05,
        stationary_drift_m: 0.0,
        update_interval_ms: 1000,
        cooldown_enabled: false,
        tips: vec!["無速限限制，適合各類定位測試使用。".into()],
    }
}

pub fn load_profiles() -> Vec<GameProfile> {
    if let Ok(profiles) = std::panic::catch_unwind(lc_core::profile::profiles) {
        if !profiles.is_empty() {
            return profiles;
        }
    }
    vec![default_bloom(), default_generic()]
}

pub fn load_profile_by_id(id: &str) -> Option<GameProfile> {
    if let Ok(Some(prof)) = std::panic::catch_unwind(|| lc_core::profile::by_id(id)) {
        return Some(prof);
    }
    load_profiles().into_iter().find(|p| p.id == id)
}
