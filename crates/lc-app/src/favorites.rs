use lc_core::LatLng;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Favorite {
    pub id: String,
    pub name: String,
    pub points: Vec<LatLng>,
    pub created_unix_ms: u64,
}

pub struct FavoritesStore {
    file_path: Option<PathBuf>,
    items: Mutex<Vec<Favorite>>,
}

impl FavoritesStore {
    pub fn new() -> Self {
        let file_path = dirs::config_dir().map(|p| p.join("bloomanywhere").join("favorites.json"));
        let mut items = Vec::new();
        if let Some(ref path) = file_path {
            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(path) {
                    if let Ok(parsed) = serde_json::from_str::<Vec<Favorite>>(&content) {
                        items = parsed;
                    }
                }
            }
        }
        Self {
            file_path,
            items: Mutex::new(items),
        }
    }
}

impl Default for FavoritesStore {
    fn default() -> Self {
        Self::new()
    }
}

impl FavoritesStore {
    pub fn list(&self) -> Vec<Favorite> {
        let lock = self.items.lock().unwrap();
        lock.clone()
    }

    pub fn add(&self, name: String, points: Vec<LatLng>) -> Vec<Favorite> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let id = uuid::Uuid::new_v4().to_string();
        let fav = Favorite {
            id,
            name,
            points,
            created_unix_ms: now,
        };

        let mut lock = self.items.lock().unwrap();
        lock.insert(0, fav);
        let copy = lock.clone();
        drop(lock);

        self.save(&copy);
        copy
    }

    pub fn delete(&self, id: &str) -> Vec<Favorite> {
        let mut lock = self.items.lock().unwrap();
        lock.retain(|f| f.id != id);
        let copy = lock.clone();
        drop(lock);

        self.save(&copy);
        copy
    }

    fn save(&self, items: &[Favorite]) {
        if let Some(ref path) = self.file_path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(items) {
                let _ = std::fs::write(path, json);
            }
        }
    }
}
