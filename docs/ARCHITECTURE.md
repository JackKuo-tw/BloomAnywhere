# BloomAnywhere — 系統架構與介面規格設計書

本文檔詳細說明 **BloomAnywhere** 跨平台 GPS 位置模擬與路徑引擎系統的架構設計、通訊協定與 HTTP / WebSocket API 規範。

---

## 1. 系統架構總覽

BloomAnywhere 採用純 Rust 開發，分為三個模組階層：

```
BloomAnywhere/
├── crates/lc-core      # 跨平台核心演算法：大地幾何數學 (Haversine/Bearing/Destination)、
│                       # 遊戲模式安全參數 (Bloom / 通用)、Mover 巡航引擎、
│                       # 擬真隨機抖動步進、Cooldown 冷卻表、GPX 1.1 處理
├── crates/lc-device    # 裝置後端抽象層：iOS (純 Rust idevice / DVT / CoreDeviceProxy)、
│                       # Android (ADB 模擬器與實機整合)、Mock 虛擬裝置
└── crates/lc-app       # 伺服器與 Web 前端 (bloomanywhere)：Axum 0.8 HTTP API、WebSocket 狀態同步、
                        # 嵌入式 Leaflet 離線地圖 UI、Clap 命令列工具
```

---

## 2. 裝置通訊協定設計 (`lc-device`)

### iOS 模擬定位通道
- **iOS 17+ 現代架構**：
  - 透過 USB/網路與 `usbmuxd` 握手。
  - 對接 `CoreDeviceProxy` 服務，建立使用者空間 TCP 通道（基於 `jktcp`）。
  - 完成 Remote Service Discovery (RSD) 握手與 TLS 加密通道建立。
  - 連線至 Instruments `com.apple.instruments.dtservicehub` 服務。
  - 開啟 DTX 通道呼叫 `LocationSimulation` 介面的 `simulateLocationWithLatitude:longitude:`。
  - 特點：完全運行於使用者空間，**不需要** root / sudo 提權驅動或系統虛擬網卡擴展。
- **iOS ≤ 16 架構**：
  - 透過 Lockdown 連接 `com.apple.dt.simulatelocation` 服務。
  - 必要時透過 `mobile_image_mounter` 掛載 Developer Disk Image。
  - 發送 4 位元組 Big-Endian 訊息型態（`0x00` 為設定座標，`0x01` 為停止模擬）。

### Android 模擬定位通道
- 自動定位環境中的 `adb` 執行檔（支援系統 PATH、`ANDROID_HOME` 與 `LC_ADB` 環境變數）。
- Android 模擬器：使用 `adb -s <serial> emu geo fix <lng> <lat>` 原生命令。
- Android 實體裝置：提供開發者選項模擬定位設定指引。

---

## 3. HTTP / WebSocket API 規格

伺服器預設啟動於 `http://127.0.0.1:8787`。所有資料格式均為 JSON。

| 方法 | 路徑 | 請求內容 | 回應結構 | 說明 |
|---|---|---|---|---|
| GET | `/api/devices` | – | `[DeviceInfo]` | 列出所有已連線裝置（含 Mock） |
| POST | `/api/connect` | `{"id": "..."}` | `AppState` | 連線至指定裝置並建立定位 Session |
| POST | `/api/disconnect` | – | `AppState` | 斷開裝置連線 |
| GET | `/api/state` | – | `AppState` | 取得目前系統狀態 |
| GET | `/api/profiles` | – | `[GameProfile]` | 取得支援之遊戲配置清單 |
| POST | `/api/profile` | `{"id": "..."}` | `AppState` | 切換遊戲配置（Bloom 為預設） |
| POST | `/api/teleport` | `{"lat": f64, "lng": f64}` | `AppState` | 瞬間傳送至指定座標（自動計算冷卻） |
| POST | `/api/route/plan` | `{"points": [LatLng], "profile": "foot"\|"bike"\|"car"\|"straight"}` | `{"path": [LatLng], "distance_m": f64}` | 道路規劃（支援 OSRM 與直線降級） |
| POST | `/api/route/start` | `{"points": [LatLng], "speed_kmh": f64, "mode": "once"\|"loop"\|"round_trip", "repeat": u32?, "realistic": bool, "snap": str}` | `AppState` | 啟動巡航引擎進行路徑走動 |
| POST | `/api/route/pause` | – | `AppState` | 暫停巡航 |
| POST | `/api/route/resume` | – | `AppState` | 繼續巡航 |
| POST | `/api/route/stop` | – | `AppState` | 停止巡航 |
| POST | `/api/speed` | `{"speed_kmh": f64}` | `AppState` | 即時調整走動速度 |
| POST | `/api/joystick` | `{"active": bool, "heading_deg": f64}` | `AppState` | 虛擬搖桿方向走動 |
| POST | `/api/clear` | – | `AppState` | 恢復手機真實 GPS 定位 |
| GET | `/api/search` | `?q=text` | `[SearchResult]` | 地名與地址搜尋（OSM Nominatim） |
| POST | `/api/gpx/import` | GPX 1.1 XML 內容 | `{"points": [LatLng]}` | 匯入 GPX 路徑 |
| POST | `/api/gpx/export` | `{"points": [LatLng], "name": str}` | GPX XML | 匯出 GPX 檔案 |
| GET | `/api/favorites` | – | `[Favorite]` | 讀取我的最愛地點與路線 |
| POST | `/api/favorites` | `{"name": str, "points": [LatLng]}` | `[Favorite]` | 新增我的最愛項目 |
| DELETE| `/api/favorites` | `?id=uuid` | `[Favorite]` | 刪除我的最愛項目 |
| GET | `/api/ws` | WebSocket 升級 | `AppState` 串流 | 狀態改變或心跳步進時即時廣播 |

### AppState 資料格式

```json
{
  "device": {
    "id": "mock",
    "name": "模擬裝置 (Mock)",
    "platform": "mock",
    "os_version": "-",
    "model": "Virtual",
    "connection": "simulated"
  },
  "connected": true,
  "position": { "lat": 25.033964, "lng": 121.564468 },
  "heading_deg": 65.0,
  "mode": "route",
  "speed_kmh": 5.5,
  "profile": {
    "id": "bloom",
    "name": "Bloom (種花模式)",
    "default_speed_kmh": 5.5,
    "max_safe_speed_kmh": 20.0,
    "cooldown_enabled": true
  },
  "route": {
    "path": [ { "lat": 25.033964, "lng": 121.564468 } ],
    "lap_progress_m": 125.4,
    "lap_length_m": 940.0,
    "total_travelled_m": 125.4,
    "lap": 0,
    "repeat": null,
    "route_mode": "loop",
    "paused": false,
    "finished": false
  },
  "cooldown": {
    "last_distance_km": 5.0,
    "total_secs": 180,
    "until_unix_ms": 1791907116264,
    "remaining_secs": 45
  },
  "pushes": 142,
  "last_error": null,
  "server_time_unix_ms": 1791907071264
}
```
