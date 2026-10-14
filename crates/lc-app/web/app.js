// BloomAnywhere Web UI Application Logic

// Global State
let map;
let ws;
let appState = null;
let activeTab = 'teleport';

// Map Layers & Markers
let userMarker = null;
let teleportTargetMarker = null;
let twoSpotStartMarker = null;
let twoSpotEndMarker = null;
let twoSpotPolyline = null;
let multiSpotMarkers = [];
let multiSpotPolyline = null;
let jumpMarkers = [];
let activeRoutePolyline = null;
let activeLeadInPolyline = null;
let flowerCircleLayer = null;

// Route & Jump Planning State
let twoSpotPlan = { start: null, end: null, plannedPath: [] };
let multiSpotPoints = [];
let multiSpotLeadIn = null;
let jumpPoints = [];
let currentJumpIndex = 0;
let autoJumpActive = false;
let autoJumpCooldownWait = false;

// Joystick State
let joystickActive = false;
let joystickHeading = 0;
let activeKeys = new Set();
let joystickDragState = { dragging: false, startX: 0, startY: 0 };

// DOM Elements
const el = (id) => document.getElementById(id);

// --- Toast Notification System ---
function showToast(title, body, type = 'info', durationMs = 4500) {
  const container = el('toastContainer');
  if (!container) return;

  const toast = document.createElement('div');
  toast.className = `toast toast-${type}`;

  const iconMap = {
    info: '💡',
    success: '✅',
    warning: '⚠️',
    error: '❌',
  };
  const icon = iconMap[type] || '📌';

  toast.innerHTML = `
    <span class="toast-icon">${icon}</span>
    <div class="toast-content">
      <div class="toast-title">${title}</div>
      <div class="toast-body">${body}</div>
    </div>
  `;

  container.appendChild(toast);

  setTimeout(() => {
    toast.style.opacity = '0';
    toast.style.transform = 'translateY(10px) scale(0.95)';
    setTimeout(() => {
      if (toast.parentNode) {
        toast.parentNode.removeChild(toast);
      }
    }, 350);
  }, durationMs);
}

// Initialize on Load
document.addEventListener('DOMContentLoaded', () => {
  initMap();
  initWebSocket();
  initEventListeners();
  loadDevices(true);
  startAutoRefreshDevices();
  loadProfiles();
  loadState();

  // Show onboarding guide if first time
  if (!localStorage.getItem('bloomanywhere_onboarding_shown')) {
    setTimeout(openOnboardingModal, 500);
  }
});

// --- Map Initialization ---
let realPosition = null;
let realUserMarker = null;

function createRealUserIcon() {
  const html = `
    <div class="real-gps-pulse-wrap">
      <div class="real-gps-dot"></div>
      <div class="real-gps-ring"></div>
    </div>
  `;
  return L.divIcon({
    html,
    className: 'custom-real-pin',
    iconSize: [24, 24],
    iconAnchor: [12, 12],
  });
}

function initMap() {
  // Center at Taipei 101 by default
  const defaultPos = [25.033964, 121.564468];
  map = L.map('map', {
    center: defaultPos,
    zoom: 16,
    zoomControl: true,
  });

  L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
    attribution: '&copy; OpenStreetMap contributors',
    maxZoom: 19,
  }).addTo(map);

  map.on('click', onMapClick);

  // Add LocateControl button to map
  const LocateControl = L.Control.extend({
    options: { position: 'topleft' },
    onAdd: function() {
      const container = L.DomUtil.create('div', 'leaflet-bar leaflet-control leaflet-control-locate');
      const link = L.DomUtil.create('a', '', container);
      link.href = '#';
      link.title = '定位到真實位置';
      link.innerHTML = '🎯';
      L.DomEvent.on(link, 'click', function(e) {
        L.DomEvent.stopPropagation(e);
        L.DomEvent.preventDefault(e);
        locateRealPosition(true, false);
      });
      return container;
    }
  });
  map.addControl(new LocateControl());

  // Automatically attempt real positioning on startup
  locateRealPosition(true, true);
}

function setRealPosition(lat, lng, centerMap, silent, source) {
  realPosition = { lat, lng };

  if (!realUserMarker) {
    realUserMarker = L.marker([lat, lng], {
      icon: createRealUserIcon(),
      zIndexOffset: 500,
    }).addTo(map);
    realUserMarker.bindPopup(`<b>🎯 目前真實位置 (${source})</b><br>${lat.toFixed(6)}, ${lng.toFixed(6)}`);
  } else {
    realUserMarker.setLatLng([lat, lng]);
    realUserMarker.setPopupContent(`<b>🎯 目前真實位置 (${source})</b><br>${lat.toFixed(6)}, ${lng.toFixed(6)}`);
  }

  if (centerMap) {
    map.flyTo([lat, lng], 16);
  }

  if (el('statusCoordVal') && el('statusCoordVal').textContent === '未設定') {
    el('statusCoordVal').textContent = `真實: ${lat.toFixed(5)}, ${lng.toFixed(5)}`;
  }

  if (!silent) {
    showToast(
      '🎯 取得真實位置',
      `來源: ${source}<br>座標: <b>${lat.toFixed(5)}, ${lng.toFixed(5)}</b>`,
      'info'
    );
  }
}

async function fallbackIpLocate(centerMap, silent) {
  try {
    const res = await fetch('/api/locate/ip');
    if (res.ok) {
      const data = await res.json();
      const cityDesc = data.city ? `${data.city}, ${data.country || ''}` : (data.country || '網路 IP');
      setRealPosition(data.lat, data.lng, centerMap, silent, `網路 IP · ${cityDesc}`);
      return true;
    }
  } catch (e) {
    console.warn('IP locate error:', e);
  }
  if (!silent) {
    showToast('⚠️ 定位失敗', '無法抓取定位，請確認已允許瀏覽器定位權限或檢查網路連線。', 'warning');
  }
  return false;
}

function locateRealPosition(centerMap = true, silent = false) {
  if (navigator.geolocation) {
    navigator.geolocation.getCurrentPosition(
      (pos) => {
        const lat = parseFloat(pos.coords.latitude.toFixed(6));
        const lng = parseFloat(pos.coords.longitude.toFixed(6));
        setRealPosition(lat, lng, centerMap, silent, 'GPS/Wi-Fi');
      },
      async (err) => {
        console.warn('Browser geolocation error:', err.message, 'falling back to IP locate...');
        await fallbackIpLocate(centerMap, silent);
      },
      { enableHighAccuracy: true, timeout: 6000, maximumAge: 0 }
    );
  } else {
    fallbackIpLocate(centerMap, silent);
  }
}

async function locateComputerPosition() {
  const btn = el('locateComputerBtn');
  const originalText = btn ? btn.innerHTML : '💻 電腦位置';
  if (btn) {
    btn.innerHTML = '⏳ 定位中...';
    btn.disabled = true;
  }

  const done = () => {
    if (btn) {
      btn.innerHTML = originalText;
      btn.disabled = false;
    }
  };

  if (!navigator.geolocation) {
    const ok = await fallbackIpLocate(true, false);
    if (ok && realPosition) {
      setTeleportTarget(realPosition.lat, realPosition.lng);
      showToast(
        '💻 電腦位置定位成功',
        `已透過網路 IP 定位至本機座標：<br><b>${realPosition.lat.toFixed(5)}, ${realPosition.lng.toFixed(5)}</b>`,
        'success'
      );
    }
    done();
    return;
  }

  navigator.geolocation.getCurrentPosition(
    (pos) => {
      const lat = parseFloat(pos.coords.latitude.toFixed(6));
      const lng = parseFloat(pos.coords.longitude.toFixed(6));
      setRealPosition(lat, lng, true, true, '電腦瀏覽器定位');
      setTeleportTarget(lat, lng);
      showToast(
        '💻 電腦位置定位成功',
        `已成功取得電腦所在座標：<br><b>${lat.toFixed(5)}, ${lng.toFixed(5)}</b><br><small style="color:#10b981;">已自動填入瞬間傳送座標！</small>`,
        'success'
      );
      done();
    },
    async (err) => {
      console.warn('Browser geolocation error:', err.message, 'trying IP locate...');
      const ipSuccess = await fallbackIpLocate(true, true);
      if (ipSuccess && realPosition) {
        setTeleportTarget(realPosition.lat, realPosition.lng);
        showToast(
          '💻 網路 IP 定位成功',
          `已依據對外 IP 定位至座標：<br><b>${realPosition.lat.toFixed(5)}, ${realPosition.lng.toFixed(5)}</b><br><small style="color:#10b981;">已自動填入瞬間傳送座標！</small>`,
          'info'
        );
      } else {
        showToast(
          '⚠️ 電腦定位失敗',
          '無法取得電腦位置，請確認瀏覽器已允許定位權限，或直接在搜尋框輸入地名。',
          'warning'
        );
      }
      done();
    },
    { enableHighAccuracy: true, timeout: 6500, maximumAge: 0 }
  );
}

function fillRealLocationToTeleport() {
  if (realPosition) {
    el('teleportLat').value = realPosition.lat;
    el('teleportLng').value = realPosition.lng;
    setTeleportTarget(realPosition.lat, realPosition.lng);
  } else {
    locateRealPosition(true, false);
    if (realPosition) {
      el('teleportLat').value = realPosition.lat;
      el('teleportLng').value = realPosition.lng;
      setTeleportTarget(realPosition.lat, realPosition.lng);
    }
  }
}


// User Marker Icon Generator (Rotates according to heading)
function createUserIcon(headingDeg = 0) {
  const html = `
    <div class="user-marker-icon">
      <svg class="user-marker-arrow" style="transform: rotate(${headingDeg}deg);" viewBox="0 0 36 36">
        <circle cx="18" cy="18" r="16" fill="#2d6a4f" fill-opacity="0.2" stroke="#2d6a4f" stroke-width="2"/>
        <path d="M18 4 L26 26 L18 21 L10 26 Z" fill="#2d6a4f" stroke="#ffffff" stroke-width="1.5"/>
      </svg>
      <span class="flower-badge-icon">🌸</span>
    </div>
  `;
  return L.divIcon({
    html,
    className: 'custom-user-pin',
    iconSize: [36, 36],
    iconAnchor: [18, 18],
  });
}

function createNumberedIcon(num, color = '#2d6a4f') {
  return L.divIcon({
    html: `<div style="background:${color};color:white;width:24px;height:24px;border-radius:50%;display:flex;align-items:center;justify-content:center;font-weight:bold;font-size:11px;border:2px solid white;box-shadow:0 2px 4px rgba(0,0,0,0.3);">${num}</div>`,
    className: 'numbered-pin',
    iconSize: [24, 24],
    iconAnchor: [12, 12],
  });
}

function createPinIcon(color = '#e63946') {
  return L.divIcon({
    html: `<div style="background:${color};width:16px;height:16px;border-radius:50%;border:3px solid white;box-shadow:0 2px 5px rgba(0,0,0,0.3);"></div>`,
    className: 'target-pin',
    iconSize: [16, 16],
    iconAnchor: [8, 8],
  });
}

// --- WebSocket Connection ---
function initWebSocket() {
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const wsUrl = `${protocol}//${window.location.host}/api/ws`;

  ws = new WebSocket(wsUrl);

  ws.onmessage = (event) => {
    try {
      const state = JSON.parse(event.data);
      updateAppState(state);
    } catch (e) {
      console.error('Error parsing WS message:', e);
    }
  };

  ws.onclose = () => {
    console.log('WS connection lost, retrying in 2 seconds...');
    setTimeout(initWebSocket, 2000);
  };

  ws.onerror = (e) => {
    console.error('WS error:', e);
  };
}

// --- State Update Handler ---
function updateAppState(state) {
  appState = state;

  // 1. Device status
  const badge = el('deviceStatusBadge');
  const toggleBtn = el('toggleConnectBtn');
  if (state.connected && state.device) {
    badge.textContent = `已連線: ${state.device.name}`;
    badge.className = 'status-badge connected';
    toggleBtn.textContent = '中斷';
    toggleBtn.className = 'btn btn-connect connected';
  } else {
    badge.textContent = '未連線';
    badge.className = 'status-badge disconnected';
    toggleBtn.textContent = '連線';
    toggleBtn.className = 'btn btn-connect';
  }

  // 2. Profile
  if (state.profile) {
    const profSelect = el('profileSelect');
    if (profSelect) {
      profSelect.value = state.profile.id;
    }
    updateProfileUI(state.profile);
  }

  // 3. Position & Marker
  if (state.position) {
    const latlng = [state.position.lat, state.position.lng];
    if (!userMarker) {
      userMarker = L.marker(latlng, { icon: createUserIcon(state.heading_deg) }).addTo(map);
    } else {
      userMarker.setLatLng(latlng);
      userMarker.setIcon(createUserIcon(state.heading_deg));
    }

    el('statusCoordVal').textContent = `${state.position.lat.toFixed(5)}, ${state.position.lng.toFixed(5)}`;
  } else {
    el('statusCoordVal').textContent = '未設定';
  }

  // 4. Status bar & Heading
  el('statusHeadingVal').textContent = `${Math.round(state.heading_deg)}°`;
  el('statusModeVal').textContent = translateMode(state.mode);
  el('statusPushesVal').textContent = state.pushes;

  // 5. Speed
  el('currentSpeedDisplay').textContent = state.speed_kmh.toFixed(1);
  el('speedSlider').value = state.speed_kmh;
  checkSpeedLimit(state.speed_kmh);

  // 6. Active Route representation
  if (state.route && state.route.path && state.route.path.length > 0) {
    const coords = state.route.path.map(p => [p.lat, p.lng]);
    if (!activeRoutePolyline) {
      activeRoutePolyline = L.polyline(coords, { color: '#2d6a4f', weight: 5, opacity: 0.8 }).addTo(map);
    } else {
      activeRoutePolyline.setLatLngs(coords);
    }

    if (state.route.lead_in && state.route.lead_in.length > 0) {
      const leadCoords = state.route.lead_in.map(p => [p.lat, p.lng]);
      if (!activeLeadInPolyline) {
        activeLeadInPolyline = L.polyline(leadCoords, {
          color: '#e76f51',
          weight: 4,
          dashArray: '6, 8',
          opacity: 0.85,
        }).addTo(map);
      } else {
        activeLeadInPolyline.setLatLngs(leadCoords);
      }
    } else if (activeLeadInPolyline) {
      map.removeLayer(activeLeadInPolyline);
      activeLeadInPolyline = null;
    }

    const lapText = state.route.repeat ? `${state.route.lap + 1} / ${state.route.repeat}` : `${state.route.lap + 1} (無限)`;
    const progressKm = (state.route.lap_progress_m / 1000).toFixed(2);
    const totalKm = (state.route.lap_length_m / 1000).toFixed(2);
    el('statusLapVal').textContent = `第 ${lapText} 圈 (${progressKm} / ${totalKm} km)`;

    // Update buttons in Two-spot and Multi-spot
    const isPaused = state.route.paused;
    el('twoSpotStartBtn').disabled = true;
    el('twoSpotPauseBtn').disabled = false;
    el('twoSpotPauseBtn').textContent = isPaused ? '▶ 繼續' : '⏸ 暫停';
    el('twoSpotStopBtn').disabled = false;

    el('multiSpotStartBtn').disabled = true;
    el('multiSpotPauseBtn').disabled = false;
    el('multiSpotPauseBtn').textContent = isPaused ? '▶ 繼續' : '⏸ 暫停';
    el('multiSpotStopBtn').disabled = false;
  } else {
    if (activeRoutePolyline) {
      map.removeLayer(activeRoutePolyline);
      activeRoutePolyline = null;
    }
    if (activeLeadInPolyline) {
      map.removeLayer(activeLeadInPolyline);
      activeLeadInPolyline = null;
    }
    el('statusLapVal').textContent = '-';

    el('twoSpotPauseBtn').disabled = true;
    el('twoSpotStopBtn').disabled = true;
    el('multiSpotPauseBtn').disabled = true;
    el('multiSpotStopBtn').disabled = true;
    if (twoSpotPlan.end) el('twoSpotStartBtn').disabled = false;
    if (multiSpotPoints.length >= 2) el('multiSpotStartBtn').disabled = false;
  }

  // 7. Cooldown widget
  updateCooldownWidget(state.cooldown);

  // 8. Auto jump handling
  if (autoJumpActive && state.cooldown && state.cooldown.remaining_secs > 0) {
    autoJumpCooldownWait = true;
    el('jumpStatusText').textContent = `⏳ 等待冷卻結束 (${state.cooldown.remaining_secs}s)...`;
  } else if (autoJumpActive && autoJumpCooldownWait) {
    // Cooldown just finished! Jump to next point!
    autoJumpCooldownWait = false;
    triggerNextJump();
  }
}

function translateMode(mode) {
  switch (mode) {
    case 'teleport': return '📍 傳送中';
    case 'route': return '🚶 路線巡航中';
    case 'joystick': return '🎮 搖桿操控中';
    default: return '閒置';
  }
}

function checkSpeedLimit(speed) {
  const warningEl = el('speedWarningText');
  const safeBadge = el('speedSafeBadge');
  const maxSafe = appState && appState.profile ? (appState.profile.max_safe_speed_kmh || 20.0) : 20.0;
  if (speed >= 100.0) {
    warningEl.classList.remove('hidden');
    warningEl.textContent = `🍄 找大蘑菇高速巡邏模式 (${speed.toFixed(1)} km/h) - 不計花朵`;
    safeBadge.textContent = `🍄 找大蘑菇模式 (${speed.toFixed(1)} km/h)`;
    safeBadge.className = 'speed-safe-badge mushroom';
  } else if (speed > maxSafe) {
    warningEl.classList.remove('hidden');
    warningEl.textContent = `⚠️ 超過 ${maxSafe} km/h 種花將停滯`;
    safeBadge.textContent = `⚠️ 超過 ${maxSafe} km/h (花朵停生)`;
    safeBadge.className = 'speed-safe-badge danger';
  } else {
    warningEl.classList.add('hidden');
    safeBadge.textContent = `🌱 安全時速中 (≤${maxSafe} km/h)`;
    safeBadge.className = 'speed-safe-badge safe';
  }
}

// --- Cooldown Widget Logic ---
let cooldownCountdownTimer = null;
let previousCooldownSecs = 0;

function playCooldownChime() {
  try {
    const AudioContext = window.AudioContext || window.webkitAudioContext;
    if (!AudioContext) return;
    const ctx = new AudioContext();
    const now = ctx.currentTime;

    // Note 1: E5 (659.25 Hz)
    const osc1 = ctx.createOscillator();
    const gain1 = ctx.createGain();
    osc1.type = 'sine';
    osc1.frequency.setValueAtTime(659.25, now);
    gain1.gain.setValueAtTime(0.12, now);
    gain1.gain.exponentialRampToValueAtTime(0.001, now + 0.35);
    osc1.connect(gain1);
    gain1.connect(ctx.destination);
    osc1.start(now);
    osc1.stop(now + 0.35);

    // Note 2: A5 (880.00 Hz)
    const osc2 = ctx.createOscillator();
    const gain2 = ctx.createGain();
    osc2.type = 'sine';
    osc2.frequency.setValueAtTime(880.00, now + 0.12);
    gain2.gain.setValueAtTime(0.18, now + 0.12);
    gain2.gain.exponentialRampToValueAtTime(0.001, now + 0.55);
    osc2.connect(gain2);
    gain2.connect(ctx.destination);
    osc2.start(now + 0.12);
    osc2.stop(now + 0.55);
  } catch (e) {
    console.debug('Chime audio error:', e);
  }
}

function notifyCooldownDone() {
  playCooldownChime();
  if ('Notification' in window) {
    if (Notification.permission === 'granted') {
      new Notification('🌸 BloomAnywhere 安全冷卻結束！', {
        body: '冷卻倒數已順利歸零，現在可以在遊戲中安全進行各項互動囉！',
        icon: '/vendor/leaflet/images/marker-icon.png'
      });
    } else if (Notification.permission !== 'denied') {
      Notification.requestPermission();
    }
  }
}

function updateCooldownWidget(cd) {
  const widget = el('cooldownWidget');
  if (!cd || cd.remaining_secs <= 0) {
    if (previousCooldownSecs > 0) {
      notifyCooldownDone();
    }
    previousCooldownSecs = 0;
    widget.classList.add('hidden');
    if (cooldownCountdownTimer) {
      clearInterval(cooldownCountdownTimer);
      cooldownCountdownTimer = null;
    }
    return;
  }

  previousCooldownSecs = cd.remaining_secs;
  widget.classList.remove('hidden');
  el('cooldownDistDisplay').textContent = cd.last_distance_km.toFixed(1);

  const formatSecs = (s) => {
    const mins = Math.floor(s / 60);
    const secs = s % 60;
    return `${String(mins).padStart(2, '0')}:${String(secs).padStart(2, '0')}`;
  };

  el('cooldownTimeDisplay').textContent = formatSecs(cd.remaining_secs);
  const pct = cd.total_secs > 0 ? (cd.remaining_secs / cd.total_secs) * 100 : 0;
  el('cooldownProgressFill').style.width = `${pct}%`;
}

// --- Map Click Router ---
function onMapClick(e) {
  const lat = parseFloat(e.latlng.lat.toFixed(6));
  const lng = parseFloat(e.latlng.lng.toFixed(6));

  switch (activeTab) {
    case 'teleport':
      setTeleportTarget(lat, lng);
      break;
    case 'twospot':
      setTwoSpotTarget(lat, lng);
      break;
    case 'multispot':
      addMultiSpotWaypoint(lat, lng);
      break;
    case 'jump':
      addJumpWaypoint(lat, lng);
      break;
    case 'joystick':
      // Move instantly to clicked point if needed
      break;
  }
}

// --- Tab 1: Teleport ---
function setTeleportTarget(lat, lng) {
  el('teleportLat').value = lat;
  el('teleportLng').value = lng;

  if (teleportTargetMarker) {
    teleportTargetMarker.setLatLng([lat, lng]);
  } else {
    teleportTargetMarker = L.marker([lat, lng], { icon: createPinIcon('#e63946') }).addTo(map);
  }

  if (appState && appState.position) {
    const distM = map.distance([appState.position.lat, appState.position.lng], [lat, lng]);
    const distKm = distM / 1000;
    const estSec = estimateCooldown(distKm);
    el('teleportDistEstimate').innerHTML = `距離當前位置: <b>${distKm.toFixed(2)} km</b> (預估冷卻: <b>${estSec > 0 ? formatSecs(estSec) : '無'}</b>)`;
  } else {
    el('teleportDistEstimate').textContent = '目標已設定，點擊「立即傳送」送出';
  }
}

function estimateCooldown(km) {
  if (km < 1.0) return 0;
  if (km <= 1.0) return 30;
  if (km <= 3.0) return 120;
  if (km <= 5.0) return 180;
  if (km <= 10.0) return 420;
  if (km <= 25.0) return 660;
  if (km <= 50.0) return 1200;
  if (km <= 100.0) return 1800;
  if (km <= 250.0) return 2700;
  if (km <= 500.0) return 3600;
  if (km <= 750.0) return 4500;
  if (km <= 1000.0) return 5400;
  return 7200;
}

function formatSecs(s) {
  const mins = Math.floor(s / 60);
  const secs = s % 60;
  return `${mins}分${secs}秒`;
}

async function execTeleport() {
  const lat = parseFloat(el('teleportLat').value);
  const lng = parseFloat(el('teleportLng').value);

  if (isNaN(lat) || isNaN(lng)) {
    alert('請輸入有效的經緯度座標');
    return;
  }

  try {
    const res = await fetch('/api/teleport', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ lat, lng }),
    });
    if (!res.ok) {
      const err = await res.json();
      alert('傳送失敗: ' + (err.error || res.statusText));
      return;
    }
    const state = await res.json();
    updateAppState(state);
  } catch (e) {
    alert('傳送連線異常: ' + e.message);
  }
}

function clearTwoSpotRoute() {
  twoSpotPlan = { start: null, end: null, plannedPath: null };
  if (twoSpotEndMarker) {
    map.removeLayer(twoSpotEndMarker);
    twoSpotEndMarker = null;
  }
  if (twoSpotPolyline) {
    map.removeLayer(twoSpotPolyline);
    twoSpotPolyline = null;
  }
  el('twoSpotStartText').textContent = '目前位置';
  el('twoSpotEndText').textContent = '尚未選擇（請點地圖）';
  el('twoSpotRouteStats').textContent = '尚未規劃路線';
  el('twoSpotStartBtn').disabled = true;
}

// --- Tab 2: Two-spot Route ---
async function setTwoSpotTarget(lat, lng) {
  // Clear any leftover multi-spot points to avoid visual clutter
  clearMultiSpotPoints();

  const currentPos = (appState && appState.position) ? appState.position : { lat: 25.033964, lng: 121.564468 };
  twoSpotPlan.start = currentPos;
  twoSpotPlan.end = { lat, lng };

  el('twoSpotStartText').textContent = `${currentPos.lat.toFixed(5)}, ${currentPos.lng.toFixed(5)}`;
  el('twoSpotEndText').textContent = `${lat.toFixed(5)}, ${lng.toFixed(5)}`;

  if (twoSpotEndMarker) {
    twoSpotEndMarker.setLatLng([lat, lng]);
  } else {
    twoSpotEndMarker = L.marker([lat, lng], { icon: createPinIcon('#2d6a4f') }).addTo(map);
  }

  await planTwoSpotRoute();
}

async function planTwoSpotRoute() {
  if (!twoSpotPlan.start || !twoSpotPlan.end) return;

  const snap = el('twoSpotSnapSelect').value;
  el('twoSpotRouteStats').textContent = '正在規劃路徑...';

  try {
    const res = await fetch('/api/route/plan', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        points: [twoSpotPlan.start, twoSpotPlan.end],
        profile: snap,
      }),
    });

    if (res.ok) {
      const data = await res.json();
      twoSpotPlan.plannedPath = data.path;

      if (twoSpotPolyline) {
        twoSpotPolyline.setLatLngs(data.path.map(p => [p.lat, p.lng]));
      } else {
        twoSpotPolyline = L.polyline(data.path.map(p => [p.lat, p.lng]), {
          color: '#52b788',
          weight: 4,
          dashArray: '6, 8',
        }).addTo(map);
      }

      el('twoSpotRouteStats').innerHTML = `路徑總長: <b>${(data.distance_m / 1000).toFixed(2)} km</b> (點數: ${data.path.length})`;
      el('twoSpotStartBtn').disabled = false;
    }
  } catch (e) {
    console.error('Route plan error:', e);
    el('twoSpotRouteStats').textContent = '路徑規劃失敗，將使用直線';
  }
}

async function startTwoSpotRoute() {
  clearMultiSpotPoints();

  if (!twoSpotPlan.plannedPath || twoSpotPlan.plannedPath.length < 2) {
    alert('請先在右側地圖點擊設定終點');
    return;
  }

  const speedKmh = parseFloat(el('speedSlider').value);
  const mode = el('twoSpotModeSelect').value;
  const realistic = el('twoSpotRealisticCheck').checked;

  try {
    const res = await fetch('/api/route/start', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        points: twoSpotPlan.plannedPath,
        speed_kmh: speedKmh,
        mode: mode,
        repeat: mode === 'once' ? null : null,
        realistic: realistic,
        snap: 'straight', // Already planned
      }),
    });
    if (res.ok) {
      const state = await res.json();
      updateAppState(state);
    }
  } catch (e) {
    alert('啟動路線失敗: ' + e.message);
  }
}

// --- Tab 3: Multi-spot Route ---
async function addMultiSpotWaypoint(lat, lng) {
  multiSpotLeadIn = null;
  multiSpotPoints.push({ lat, lng });
  renderMultiSpotList();
  await planMultiSpotRoute();
}

function renderMultiSpotList() {
  const list = el('waypointListBox');
  list.innerHTML = '';

  // Clear previous markers
  multiSpotMarkers.forEach(m => map.removeLayer(m));
  multiSpotMarkers = [];

  if (multiSpotPoints.length === 0) {
    list.innerHTML = '<div class="empty-list-hint">點擊地圖添加巡航途經點...</div>';
    el('multiSpotStartBtn').disabled = true;
    el('exportGpxBtn').disabled = true;
    el('saveMultiRouteFavBtn').disabled = true;
    if (multiSpotPolyline) {
      map.removeLayer(multiSpotPolyline);
      multiSpotPolyline = null;
    }
    return;
  }

  multiSpotPoints.forEach((pt, index) => {
    // Marker
    const marker = L.marker([pt.lat, pt.lng], { icon: createNumberedIcon(index + 1) }).addTo(map);
    multiSpotMarkers.push(marker);

    // List item
    const item = document.createElement('div');
    item.className = 'waypoint-item';
    item.innerHTML = `
      <div class="waypoint-label">
        <span class="waypoint-badge">${index + 1}</span>
        <span class="waypoint-coords">${pt.lat.toFixed(5)}, ${pt.lng.toFixed(5)}</span>
      </div>
      <button class="del-wp-btn" title="刪除此點" onclick="deleteMultiSpotPoint(${index})">&times;</button>
    `;
    list.appendChild(item);
  });

  const leadInNote = (multiSpotLeadIn && multiSpotLeadIn.length >= 2) ? ' (含起步導引)' : '';
  el('multiSpotRouteStats').textContent = `途經點數: ${multiSpotPoints.length}${leadInNote}`;
  el('exportGpxBtn').disabled = false;
  el('saveMultiRouteFavBtn').disabled = false;
  if (multiSpotPoints.length >= 2) {
    el('multiSpotStartBtn').disabled = false;
  }
}

window.deleteMultiSpotPoint = async function(index) {
  multiSpotLeadIn = null;
  multiSpotPoints.splice(index, 1);
  renderMultiSpotList();
  await planMultiSpotRoute();
};

function clearMultiSpotPoints() {
  multiSpotLeadIn = null;
  multiSpotPoints = [];
  renderMultiSpotList();
}

async function planMultiSpotRoute() {
  if (multiSpotPoints.length < 2) {
    if (multiSpotPolyline) {
      map.removeLayer(multiSpotPolyline);
      multiSpotPolyline = null;
    }
    return;
  }

  const snap = el('multiSpotSnapSelect').value;
  try {
    const res = await fetch('/api/route/plan', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        points: multiSpotPoints,
        profile: snap,
      }),
    });

    if (res.ok) {
      const data = await res.json();
      if (multiSpotPolyline) {
        multiSpotPolyline.setLatLngs(data.path.map(p => [p.lat, p.lng]));
      } else {
        multiSpotPolyline = L.polyline(data.path.map(p => [p.lat, p.lng]), {
          color: '#52b788',
          weight: 4,
          dashArray: '6, 8',
        }).addTo(map);
      }
      el('multiSpotRouteStats').innerHTML = `途經點: ${multiSpotPoints.length} | 總長: <b>${(data.distance_m / 1000).toFixed(2)} km</b>`;
    }
  } catch (e) {
    console.error('Multi-spot plan error:', e);
  }
}

async function startMultiSpotRoute() {
  if (multiSpotPoints.length < 2) return;

  const speedKmh = parseFloat(el('speedSlider').value);
  const mode = el('multiSpotModeSelect').value;
  const realistic = el('multiSpotRealisticCheck').checked;
  const snap = el('multiSpotSnapSelect').value;

  const payload = {
    points: multiSpotPoints,
    speed_kmh: speedKmh,
    mode: mode,
    realistic: realistic,
    snap: snap,
  };
  if (multiSpotLeadIn && multiSpotLeadIn.length >= 2) {
    payload.lead_in = multiSpotLeadIn;
  }

  try {
    const res = await fetch('/api/route/start', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    });
    if (res.ok) {
      const state = await res.json();
      updateAppState(state);
    }
  } catch (e) {
    alert('啟動巡航失敗: ' + e.message);
  }
}

// --- Tab 4: Jump Teleport ---
function addJumpWaypoint(lat, lng) {
  jumpPoints.push({ lat, lng });
  renderJumpList();
}

function renderJumpList() {
  const list = el('jumpListBox');
  list.innerHTML = '';

  jumpMarkers.forEach(m => map.removeLayer(m));
  jumpMarkers = [];

  if (jumpPoints.length === 0) {
    list.innerHTML = '<div class="empty-list-hint">點擊地圖添加跳躍站點...</div>';
    el('jumpNextBtn').disabled = true;
    el('jumpStopBtn').disabled = true;
    el('jumpStatusText').textContent = '尚無跳躍點';
    return;
  }

  jumpPoints.forEach((pt, index) => {
    const isCurrent = index === currentJumpIndex;
    const color = isCurrent ? '#ffb703' : '#457b9d';
    const marker = L.marker([pt.lat, pt.lng], { icon: createNumberedIcon(index + 1, color) }).addTo(map);
    jumpMarkers.push(marker);

    const item = document.createElement('div');
    item.className = `waypoint-item ${isCurrent ? 'active' : ''}`;
    item.innerHTML = `
      <div class="waypoint-label">
        <span class="waypoint-badge" style="background:${color}">${index + 1}</span>
        <span class="waypoint-coords">${pt.lat.toFixed(5)}, ${pt.lng.toFixed(5)}</span>
      </div>
      <button class="btn-sm btn-outline" onclick="jumpToPoint(${index})">跳此點</button>
    `;
    list.appendChild(item);
  });

  el('jumpStatusText').textContent = `站點進度: ${currentJumpIndex + 1} / ${jumpPoints.length}`;
  el('jumpNextBtn').disabled = false;
  el('jumpStopBtn').disabled = !autoJumpActive;
}

window.jumpToPoint = async function(index) {
  if (index >= jumpPoints.length) return;
  currentJumpIndex = index;
  renderJumpList();

  const pt = jumpPoints[index];
  try {
    const res = await fetch('/api/teleport', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(pt),
    });
    if (res.ok) {
      const state = await res.json();
      updateAppState(state);
    }
  } catch (e) {
    console.error('Jump failed:', e);
  }
};

function triggerNextJump() {
  if (currentJumpIndex + 1 < jumpPoints.length) {
    jumpToPoint(currentJumpIndex + 1);
  } else {
    // Finished all jump points!
    autoJumpActive = false;
    renderJumpList();
    alert('🎉 已完成所有站點的跳躍傳送！');
  }
}

function clearJumpPoints() {
  jumpPoints = [];
  currentJumpIndex = 0;
  autoJumpActive = false;
  renderJumpList();
}

// --- Tab 5: Virtual Joystick ---
function initJoystick() {
  const base = el('joystickBase');
  const thumb = el('joystickThumb');

  const onStart = (e) => {
    joystickDragState.dragging = true;
    const rect = base.getBoundingClientRect();
    joystickDragState.centerX = rect.left + rect.width / 2;
    joystickDragState.centerY = rect.top + rect.height / 2;
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onEnd);
    document.addEventListener('touchmove', onMove, { passive: false });
    document.addEventListener('touchend', onEnd);
  };

  const onMove = (e) => {
    if (!joystickDragState.dragging) return;
    if (e.cancelable) e.preventDefault();

    const clientX = e.touches ? e.touches[0].clientX : e.clientX;
    const clientY = e.touches ? e.touches[0].clientY : e.clientY;

    const dx = clientX - joystickDragState.centerX;
    const dy = clientY - joystickDragState.centerY;
    const dist = Math.sqrt(dx * dx + dy * dy);
    const maxRadius = 50;

    const clampedDist = Math.min(dist, maxRadius);
    const rad = Math.atan2(dy, dx);

    const thumbX = Math.cos(rad) * clampedDist;
    const thumbY = Math.sin(rad) * clampedDist;

    thumb.style.transform = `translate(${thumbX}px, ${thumbY}px)`;

    // Calculate heading (0 = North, 90 = East, 180 = South, 270 = West)
    let heading = (Math.atan2(dx, -dy) * 180 / Math.PI + 360) % 360;
    sendJoystickCommand(true, heading);
  };

  const onEnd = () => {
    joystickDragState.dragging = false;
    thumb.style.transform = 'translate(0px, 0px)';
    document.removeEventListener('mousemove', onMove);
    document.removeEventListener('mouseup', onEnd);
    document.removeEventListener('touchmove', onMove);
    document.removeEventListener('touchend', onEnd);
    sendJoystickCommand(false, joystickHeading);
  };

  base.addEventListener('mousedown', onStart);
  base.addEventListener('touchstart', onStart, { passive: false });

  // D-Pad buttons
  document.querySelectorAll('.dpad-btn[data-dir]').forEach(btn => {
    const dir = parseFloat(btn.dataset.dir);
    btn.addEventListener('mousedown', () => sendJoystickCommand(true, dir));
    btn.addEventListener('mouseup', () => sendJoystickCommand(false, dir));
    btn.addEventListener('touchstart', (e) => { e.preventDefault(); sendJoystickCommand(true, dir); });
    btn.addEventListener('touchend', (e) => { e.preventDefault(); sendJoystickCommand(false, dir); });
  });

  el('dpadStop').addEventListener('click', () => sendJoystickCommand(false, joystickHeading));

  // Keyboard navigation (WASD & Arrows)
  window.addEventListener('keydown', handleKeyDown);
  window.addEventListener('keyup', handleKeyUp);
}

let lastJoystickSend = 0;
async function sendJoystickCommand(active, heading) {
  joystickActive = active;
  joystickHeading = heading;

  el('joystickHeadingDisplay').textContent = `${Math.round(heading)}°`;
  el('joystickActiveDisplay').textContent = active ? '移動中' : '閒置中';
  el('joystickActiveDisplay').style.color = active ? '#2d6a4f' : '#6b7280';

  const now = Date.now();
  if (!active || (now - lastJoystickSend > 150)) {
    lastJoystickSend = now;
    try {
      await fetch('/api/joystick', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ active, heading_deg: heading }),
      });
    } catch (e) {
      console.error('Joystick send error:', e);
    }
  }
}

function handleKeyDown(e) {
  if (['input', 'textarea', 'select'].includes(document.activeElement.tagName.toLowerCase())) return;

  // Space: Pause/Resume active route
  if (e.code === 'Space') {
    e.preventDefault();
    toggleRoutePauseResume();
    return;
  }

  // Bracket keys: adjust speed
  if (e.code === 'BracketLeft') {
    e.preventDefault();
    adjustSpeedDelta(-0.5);
    return;
  }
  if (e.code === 'BracketRight') {
    e.preventDefault();
    adjustSpeedDelta(0.5);
    return;
  }

  // ?: Help & Safety Guide
  if (e.key === '?' || (e.shiftKey && e.code === 'Slash')) {
    e.preventDefault();
    openSafetyGuideModal();
    return;
  }

  // Esc: Close any open modal
  if (e.code === 'Escape') {
    closeAllModals();
    return;
  }

  // 1 ~ 5: Speed presets
  if (['Digit1', 'Digit2', 'Digit3', 'Digit4', 'Digit5'].includes(e.code)) {
    const idx = parseInt(e.code.replace('Digit', '')) - 1;
    const presets = document.querySelectorAll('#speedPresetsContainer .preset-btn');
    if (presets[idx]) {
      presets[idx].click();
    }
    return;
  }

  const validKeys = ['KeyW', 'KeyA', 'KeyS', 'KeyD', 'ArrowUp', 'ArrowLeft', 'ArrowDown', 'ArrowRight'];
  if (validKeys.includes(e.code)) {
    e.preventDefault();
    activeKeys.add(e.code);
    evaluateActiveKeys();
  }
}

async function toggleRoutePauseResume() {
  if (!appState || !appState.route) return;
  const endpoint = appState.route.paused ? '/api/route/resume' : '/api/route/pause';
  try {
    const res = await fetch(endpoint, { method: 'POST' });
    if (res.ok) updateAppState(await res.json());
  } catch (err) {
    console.error('Failed to toggle pause/resume:', err);
  }
}

function adjustSpeedDelta(delta) {
  const slider = el('speedSlider');
  const current = parseFloat(slider.value) || 5.5;
  const max = parseFloat(slider.max) || 120;
  const next = Math.max(1, Math.min(max, current + delta));
  slider.value = next;
  el('currentSpeedDisplay').textContent = next.toFixed(1);
  setSpeed(next);
}

function closeAllModals() {
  closeFlowerLoopModal();
  closeSafeRestoreModal();
  closeSafetyGuideModal();
  closeFavoritesModal();
  closeOnboardingModal();
}

function handleKeyUp(e) {
  if (activeKeys.has(e.code)) {
    activeKeys.delete(e.code);
    evaluateActiveKeys();
  }
}

function evaluateActiveKeys() {
  if (activeKeys.size === 0) {
    sendJoystickCommand(false, joystickHeading);
    return;
  }

  let y = 0; // Up is +1, Down is -1
  let x = 0; // Right is +1, Left is -1

  if (activeKeys.has('KeyW') || activeKeys.has('ArrowUp')) y += 1;
  if (activeKeys.has('KeyS') || activeKeys.has('ArrowDown')) y -= 1;
  if (activeKeys.has('KeyD') || activeKeys.has('ArrowRight')) x += 1;
  if (activeKeys.has('KeyA') || activeKeys.has('ArrowLeft')) x -= 1;

  if (x === 0 && y === 0) {
    sendJoystickCommand(false, joystickHeading);
    return;
  }

  // Heading calculation (0 = N, 90 = E, 180 = S, 270 = W)
  let heading = (Math.atan2(x, y) * 180 / Math.PI + 360) % 360;
  sendJoystickCommand(true, heading);
}

// --- Flower Loop Generator & Calculator ---
let flowerPreviewCircle = null;

function updateFlowerLoopCalculator() {
  const radius = parseFloat(el('flowerRadiusInput').value) || 150;
  const speed = (appState && appState.speed_kmh) ? appState.speed_kmh : (parseFloat(el('speedSlider').value) || 5.5);
  const circumference = 2 * Math.PI * radius;
  const speedMs = (speed * 1000.0) / 3600.0;
  const estTimeSecs = speedMs > 0 ? (circumference / speedMs) : 0;

  const mins = Math.floor(estTimeSecs / 60);
  const secs = Math.round(estTimeSecs % 60);

  el('flowerCircumferenceText').textContent = `約 ${Math.round(circumference)} 公尺`;
  el('flowerLapTimeText').textContent = `約 ${mins} 分 ${secs} 秒 (時速 ${speed.toFixed(1)} km/h)`;

  const badge = el('flowerCooldownBadge');
  if (estTimeSecs >= 300) {
    badge.className = 'flower-cooldown-badge safe';
    badge.innerHTML = `✅ 單圈超過 5 分鐘 (${mins}分${secs}秒)！符合網格種花冷卻重置機制，花瓣播種效率 100%！`;
  } else {
    const minSafeRadius = Math.ceil((300 * speedMs) / (2 * Math.PI));
    badge.className = 'flower-cooldown-badge warning';
    badge.innerHTML = `⚠️ 單圈僅 ${mins}分${secs}秒（小於 5 分鐘冷卻期）！同一網格 5 分鐘內無法重複播種，將導致花瓣空燒浪費！建議將半徑擴大至至少 <b>${minSafeRadius} 公尺</b> 或調慢時速。`;
  }

  // Update map preview circle
  const center = (appState && appState.position) ? appState.position : map.getCenter();
  if (!flowerPreviewCircle) {
    flowerPreviewCircle = L.circle(center, {
      radius: radius,
      color: '#f72585',
      weight: 2,
      dashArray: '6, 6',
      fillColor: '#f72585',
      fillOpacity: 0.12,
    }).addTo(map);
  } else {
    flowerPreviewCircle.setLatLng(center);
    flowerPreviewCircle.setRadius(radius);
  }
}

function openFlowerLoopModal() {
  updateFlowerLoopCalculator();
  el('flowerLoopModal').classList.remove('hidden');
}

function closeFlowerLoopModal() {
  el('flowerLoopModal').classList.add('hidden');
  if (flowerPreviewCircle) {
    map.removeLayer(flowerPreviewCircle);
    flowerPreviewCircle = null;
  }
}

function generateCirclePoints(center, radiusM, numPoints = 18) {
  const points = [];
  const earthRadius = 6371008.8;
  const latRad = center.lat * Math.PI / 180;

  for (let i = 0; i < numPoints; i++) {
    const angle = (2 * Math.PI * i) / numPoints;
    const dLat = (radiusM * Math.cos(angle)) / earthRadius;
    const dLng = (radiusM * Math.sin(angle)) / (earthRadius * Math.cos(latRad));

    const pLat = center.lat + (dLat * 180 / Math.PI);
    const pLng = center.lng + (dLng * 180 / Math.PI);
    points.push({ lat: parseFloat(pLat.toFixed(6)), lng: parseFloat(pLng.toFixed(6)) });
  }
  return points;
}

async function applyFlowerLoop() {
  const radius = parseFloat(el('flowerRadiusInput').value) || 150;
  const count = parseInt(el('flowerPointsCountInput').value) || 18;

  const center = (appState && appState.position) ? appState.position : map.getCenter();
  const circlePoints = generateCirclePoints(center, radius, count);

  closeFlowerLoopModal();

  // Populate into Multi-spot Route
  multiSpotPoints = circlePoints;
  // Lead-in approach path from center to circumference: ZERO instant teleportation!
  multiSpotLeadIn = [
    { lat: center.lat, lng: center.lng },
    { lat: circlePoints[0].lat, lng: circlePoints[0].lng }
  ];
  switchTab('multispot');
  renderMultiSpotList();
  el('multiSpotModeSelect').value = 'loop';
  el('multiSpotSnapSelect').value = 'straight';
  el('speedSlider').value = 5.5;
  el('currentSpeedDisplay').textContent = '5.5';

  // Automatically start loop walk!
  await startMultiSpotRoute();
}

// --- Safe Restore Modal Logic (Anti-Rubberband) ---
function openSafeRestoreModal() {
  el('confirmGameClosedCheckbox').checked = false;
  el('confirmRestoreBtn').disabled = true;
  el('safeRestoreModal').classList.remove('hidden');
}

function closeSafeRestoreModal() {
  el('safeRestoreModal').classList.add('hidden');
}

async function executeSafeRestore() {
  closeSafeRestoreModal();
  try {
    const res = await fetch('/api/clear', { method: 'POST' });
    if (res.ok) {
      const state = await res.json();
      updateAppState(state);
      locateRealPosition(true, false);
    }
  } catch (e) {
    alert('清除定位失敗: ' + e.message);
  }
}

// --- Safety & Anti-Ban Guide ---
const guideContents = {
  bloom: `
    <div class="guide-card">
      <h4>🌸 1. 嚴格遵守種花速限 (≤ 20 km/h)</h4>
      <p>種花遊戲底層對播種花朵有嚴格時速限制：</p>
      <ul>
        <li><b>0 ~ 15 km/h</b>：最佳培育效率區間（散步 4.5、快走 6.5、慢跑 12 km/h），花瓣消耗正常且播種密度最高。</li>
        <li><b>15 ~ 20 km/h</b>：臨界速度，稍微顛簸就可能超標。</li>
        <li><b>&gt; 20 km/h</b>：<b>立刻強制停止種花！</b> 遊戲跳出「移動速度過快」警示，嚴重者將被記錄異常。</li>
      </ul>
    </div>
    <div class="guide-card">
      <h4>🔄 2. 同區域「5 分鐘冷卻」機制（避開花瓣浪費）</h4>
      <p>遊戲機制規定：同一個 S2 網格在 5 分鐘內無法重複種出花朵。若繞圈太小（例如半徑 100m 且時速 15 km/h，單圈僅 1.5 分鐘），後續圈數的花瓣將直接白白蒸發！</p>
      <ul>
        <li>使用內建「繞圈種花路線生成器」，軟體會依當前時速自動計算單圈時間。</li>
        <li>確保單圈耗時 <b>≥ 5 分鐘</b>，才能循環達到 100% 播種效率。</li>
      </ul>
    </div>
    <div class="guide-card">
      <h4>🚨 3. 絕對杜絕「橡皮筋回彈 (Rubberbanding)」</h4>
      <p><b>這是飛人被偵測封禁的第一大死因！</b></p>
      <ul>
        <li>若手機遊戲 App 仍在開啟狀態下拔除傳輸線或點擊「還原真實定位」，手機定位會以超音速「瞬間跳回」真實地址。</li>
        <li><b>黃金法則：拔線或還原定位前，務必先將手機上的遊戲 App 完全滑掉退出！</b></li>
      </ul>
    </div>
    <div class="guide-card">
      <h4>🌿 4. 路線擬真與路網貼齊</h4>
      <ul>
        <li>盡量使用「兩點/多點路線」搭配「步行路網貼齊 (OSRM)」，走真實的道路、人行道與斑馬線。</li>
        <li>避免直線穿過大樓、河流、湖泊或禁入區域，演算法可從幾何碰撞輕易辨識非真人移動。</li>
      </ul>
    </div>
  `,
  cooldown: `
    <div class="guide-card">
      <h4>⏳ 冷卻時間表 (Cooldown Rules)</h4>
      <p>社群多年實測驗證，距離跳躍後所需安全靜置時間：</p>
      <table class="guide-table">
        <thead>
          <tr><th>跳躍距離</th><th>安全冷卻時間</th><th>跳躍距離</th><th>安全冷卻時間</th></tr>
        </thead>
        <tbody>
          <tr><td>1 km</td><td>30 秒</td><td>40 km</td><td>22 分鐘</td></tr>
          <tr><td>5 km</td><td>2 分鐘</td><td>100 km</td><td>30 分鐘</td></tr>
          <tr><td>10 km</td><td>7 分鐘</td><td>250 km</td><td>45 分鐘</td></tr>
          <tr><td>20 km</td><td>12 分鐘</td><td>500 km</td><td>65 分鐘</td></tr>
          <tr><td>30 km</td><td>18 分鐘</td><td>≥ 1300 km</td><td>120 分鐘 (上限)</td></tr>
        </tbody>
      </table>
    </div>
    <div class="guide-card">
      <h4>⚠️ 核心原則：傳送不觸發懲罰，傳送後「立刻互動」才會！</h4>
      <ul>
        <li><b>會觸發軟鎖的行為</b>：加入蘑菇戰鬥、採摘花朵、領取探險成果等具有伺服器互動記錄之操作。</li>
        <li><b>不會觸發冷卻的行為</b>：在地圖上縮放檢視、整理背包、升級探險夥伴、領取任務成就。</li>
        <li><b>建議作法</b>：遠程跳躍後，讓手機保持冷卻計時完畢（本軟體會發出音效提醒），再開始遊戲操作。</li>
      </ul>
    </div>
  `,
  steps: `
    <div class="guide-card">
      <h4>🚶 步數與 GPS 的關鍵差異（破解花苗不長、伺服器異常）</h4>
      <p>許多使用者困惑：「為什麼我 GPS 跑了 10 公里，花苗步數卻完全沒增加？」</p>
      <ul>
        <li><b>GPS 虛擬定位負責</b>：地圖移動、消耗花瓣種花、觸發巨花綻放、偵測周邊蘑菇與探險苗。</li>
        <li><b>手機動態感應器 (CoreMotion / Google Fit) 負責</b>：花苗成長步數、每日行走步數統計、探險隊伍好感度步數。</li>
      </ul>
    </div>
    <div class="guide-card">
      <h4>🛡️ 如何讓步數與 GPS 完美同步防封？</h4>
      <ul>
        <li><b>嚴重警訊</b>：若帳號在 1 小時內種了 10,000 朵花（GPS 位移 10km），但手機健康步數增加為 0，伺服器極易標記為位置注入外掛。</li>
        <li><b>推薦配套</b>：
          <br>1. <b>實體搖步器（擺步機）</b>：放置手機自動搖動，產生自然的加速度感應器步數。
          <br>2. <b>手持手機輕搖</b>：在使用搖桿或路徑巡航時，將手機拿在手中自然晃動。
          <br>3. <b>健康 App 同步</b>：iOS「健康」或 Android「Health Connect / Defit」，確保步數與定位里程合理匹配。
        </li>
      </ul>
    </div>
  `,
  hotkeys: `
    <div class="guide-card">
      <h4>⌨️ 全域鍵盤快捷鍵一覽</h4>
      <table class="guide-table">
        <thead>
          <tr><th>按鍵</th><th>功能描述</th></tr>
        </thead>
        <tbody>
          <tr><td><span class="hotkey-badge">W</span> <span class="hotkey-badge">A</span> <span class="hotkey-badge">S</span> <span class="hotkey-badge">D</span> / 方向鍵</td><td>即時操控虛擬搖桿移動方向</td></tr>
          <tr><td><span class="hotkey-badge">Space (空白鍵)</span></td><td>暫停 / 繼續當前路徑巡航</td></tr>
          <tr><td><span class="hotkey-badge">1</span> ~ <span class="hotkey-badge">5</span></td><td>快速切換移動時速預設檔位 (散步 4.5 ~ 速巡 120 km/h)</td></tr>
          <tr><td><span class="hotkey-badge">[</span> / <span class="hotkey-badge">]</span></td><td>即時微調移動時速 (±0.5 km/h)</td></tr>
          <tr><td><span class="hotkey-badge">Esc</span></td><td>關閉當前開啟的彈出視窗</td></tr>
          <tr><td><span class="hotkey-badge">?</span></td><td>快速開啟本防封手冊與快捷鍵百科</td></tr>
        </tbody>
      </table>
    </div>
  `
};

function switchGuideTab(tabKey) {
  document.querySelectorAll('.guide-tab-btn').forEach(b => {
    b.classList.toggle('active', b.dataset.guidetab === tabKey);
  });
  el('guideTabContent').innerHTML = guideContents[tabKey] || '';
}

function openSafetyGuideModal() {
  switchGuideTab('bloom');
  el('safetyGuideModal').classList.remove('hidden');
}

function closeSafetyGuideModal() {
  el('safetyGuideModal').classList.add('hidden');
}

// --- Profile Handling ---
async function loadProfiles() {
  try {
    const res = await fetch('/api/profiles');
    if (res.ok) {
      const list = await res.json();
      const select = el('profileSelect');
      if (select) {
        select.innerHTML = '';
        list.forEach(p => {
          const opt = document.createElement('option');
          opt.value = p.id;
          opt.textContent = `${p.id === 'bloom' ? '🌸 ' : ''}${p.name}`;
          select.appendChild(opt);
        });
      }
      if (list.length > 0) {
        const defaultProf = list.find(p => p.id === 'bloom') || list[0];
        updateProfileUI(defaultProf);
      }
    }
  } catch (e) {
    console.error('Failed to load profiles:', e);
  }
}

function updateProfileUI(profile) {
  const presetsContainer = el('speedPresetsContainer');
  presetsContainer.innerHTML = '';

  el('speedSlider').max = profile.max_speed_kmh;

  profile.speed_presets.forEach(preset => {
    const btn = document.createElement('button');
    btn.className = 'preset-btn';
    btn.textContent = `${preset.label} ${preset.kmh}`;
    btn.onclick = () => {
      el('speedSlider').value = preset.kmh;
      setSpeed(preset.kmh);
    };
    presetsContainer.appendChild(btn);
  });

  if (profile.tips && profile.tips.length > 0) {
    const tipEl = el('bloomTipText');
    if (tipEl) {
      tipEl.textContent = profile.tips[0];
    }
  }
}

async function setSpeed(kmh) {
  el('currentSpeedDisplay').textContent = kmh.toFixed(1);
  checkSpeedLimit(kmh);
  try {
    const res = await fetch('/api/speed', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ speed_kmh: kmh }),
    });
    if (res.ok) {
      const state = await res.json();
      updateAppState(state);
    }
  } catch (e) {
    console.error('Failed to set speed:', e);
  }
}

// --- Device Connection & Background Auto-Detection ---
let knownDeviceIds = new Set();
let autoRefreshTimer = null;

async function loadDevices(silent = false) {
  try {
    const res = await fetch('/api/devices');
    if (!res.ok) return;
    const devices = await res.json();
    const select = el('deviceSelect');
    if (!select) return;

    const currentVal = select.value;
    let newlyDetected = [];

    devices.forEach(d => {
      if (!knownDeviceIds.has(d.id)) {
        newlyDetected.push(d);
        knownDeviceIds.add(d.id);
      }
    });

    select.innerHTML = '';

    if (devices.length === 0) {
      select.innerHTML = '<option value="">未偵測到裝置 (請以傳輸線連接手機)</option>';
      knownDeviceIds.clear();
      return;
    }

    devices.forEach(d => {
      const opt = document.createElement('option');
      opt.value = d.id;
      opt.textContent = `${d.name} (${d.platform})`;
      select.appendChild(opt);
    });

    // Auto-select if nothing selected or previous selection gone
    if (currentVal && devices.some(d => d.id === currentVal)) {
      select.value = currentVal;
    } else {
      select.value = devices[0].id;
    }

    // Notify user if a new physical device is detected
    if (newlyDetected.length > 0 && !silent) {
      const d = newlyDetected[0];
      showToast(
        '📱 已自動偵測到手機裝置！',
        `已自動選取：<b>${d.name}</b><br><small style="color:#d97706;">🔑 提醒：若手機螢幕鎖定，請先在手機上解鎖輸入密碼並點選「信任此電腦」再點擊連線。</small>`,
        'success',
        7000
      );
    }
  } catch (e) {
    if (!silent) console.error('Failed to load devices:', e);
  }
}

function startAutoRefreshDevices() {
  if (autoRefreshTimer) clearInterval(autoRefreshTimer);
  autoRefreshTimer = setInterval(() => {
    // Only auto-refresh in background if currently disconnected
    if (!appState || !appState.connected) {
      loadDevices(false);
    }
  }, 3500);
}

async function toggleConnect() {
  if (appState && appState.connected) {
    // Disconnect
    try {
      const res = await fetch('/api/disconnect', { method: 'POST' });
      if (res.ok) {
        const state = await res.json();
        updateAppState(state);
        showToast('🔌 已中斷連線', '已停止定位模擬通道。', 'info');
      }
    } catch (e) {
      alert('中斷連線失敗: ' + e.message);
    }
  } else {
    // Connect
    const id = el('deviceSelect').value;
    if (!id) {
      alert('未偵測到裝置，請先以傳輸線連接手機');
      return;
    }

    try {
      const res = await fetch('/api/connect', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ id }),
      });
      if (res.ok) {
        const state = await res.json();
        updateAppState(state);
        showToast('✅ 裝置連線成功！', '已成功建立定位模擬通道，可開始進行地圖巡航！', 'success');
      } else {
        const err = await res.json();
        alert('連線失敗: ' + (err.error || res.statusText));
      }
    } catch (e) {
      alert('連線錯誤: ' + e.message);
    }
  }
}

// --- First-Time User Onboarding Tutorial ---
let currentOnboardingStep = 1;
const TOTAL_ONBOARDING_STEPS = 4;

function openOnboardingModal() {
  currentOnboardingStep = 1;
  showOnboardingStep(1);
  el('onboardingModal').classList.remove('hidden');
}

function closeOnboardingModal() {
  el('onboardingModal').classList.add('hidden');
  localStorage.setItem('bloomanywhere_onboarding_shown', 'true');
}

function showOnboardingStep(step) {
  currentOnboardingStep = step;
  document.querySelectorAll('.onboarding-step').forEach(s => {
    s.classList.toggle('active', parseInt(s.dataset.step) === step);
  });
  document.querySelectorAll('.onboarding-dots .dot').forEach(d => {
    d.classList.toggle('active', parseInt(d.dataset.step) === step);
  });

  const prevBtn = el('prevOnboardingBtn');
  const nextBtn = el('nextOnboardingBtn');
  const finishBtn = el('finishOnboardingBtn');

  if (prevBtn) prevBtn.classList.toggle('hidden', step === 1);
  if (nextBtn) nextBtn.classList.toggle('hidden', step === TOTAL_ONBOARDING_STEPS);
  if (finishBtn) finishBtn.classList.toggle('hidden', step !== TOTAL_ONBOARDING_STEPS);
}

function nextOnboardingStep() {
  if (currentOnboardingStep < TOTAL_ONBOARDING_STEPS) {
    showOnboardingStep(currentOnboardingStep + 1);
  }
}

function prevOnboardingStep() {
  if (currentOnboardingStep > 1) {
    showOnboardingStep(currentOnboardingStep - 1);
  }
}

// --- Restore Real GPS ---
async function restoreRealLocation() {
  if (!confirm('確定要清除模擬並還原手機的真實 GPS 定位嗎？')) return;

  try {
    const res = await fetch('/api/clear', { method: 'POST' });
    if (res.ok) {
      const state = await res.json();
      updateAppState(state);
      // Immediately locate and center real position on map!
      locateRealPosition(true, false);
    }
  } catch (e) {
    alert('清除定位失敗: ' + e.message);
  }
}

// --- Search / Nominatim ---
let searchDebounce = null;
function onSearchInput(e) {
  const query = e.target.value.trim();
  clearTimeout(searchDebounce);
  const resultsDropdown = el('searchResults');

  if (query.length < 2) {
    resultsDropdown.classList.add('hidden');
    return;
  }

  searchDebounce = setTimeout(async () => {
    try {
      const res = await fetch(`/api/search?q=${encodeURIComponent(query)}`);
      if (res.ok) {
        const items = await res.json();
        resultsDropdown.innerHTML = '';
        if (items.length === 0) {
          resultsDropdown.innerHTML = '<div class="search-result-item text-muted">無符合結果</div>';
        } else {
          items.forEach(item => {
            const div = document.createElement('div');
            div.className = 'search-result-item';
            div.textContent = item.name;
            div.onclick = () => {
              resultsDropdown.classList.add('hidden');
              el('searchInput').value = item.name;
              map.flyTo([item.lat, item.lng], 16);
              setTeleportTarget(item.lat, item.lng);
            };
            resultsDropdown.appendChild(div);
          });
        }
        resultsDropdown.classList.remove('hidden');
      }
    } catch (err) {
      console.error('Search error:', err);
    }
  }, 350);
}

// --- GPX Import & Export ---
async function importGpxFile(e) {
  const file = e.target.files[0];
  if (!file) return;

  const text = await file.text();
  try {
    const res = await fetch('/api/gpx/import', {
      method: 'POST',
      headers: { 'Content-Type': 'application/xml' },
      body: text,
    });
    if (res.ok) {
      const data = await res.json();
      if (data.points && data.points.length > 0) {
        multiSpotLeadIn = null;
        multiSpotPoints = data.points;
        renderMultiSpotList();
        await planMultiSpotRoute();
        map.fitBounds(L.latLngBounds(data.points.map(p => [p.lat, p.lng])));
        alert(`✅ 成功匯入 ${data.points.length} 個 GPX 座標點！`);
      }
    } else {
      alert('GPX 格式解析失敗');
    }
  } catch (err) {
    alert('匯入失敗: ' + err.message);
  }
}

async function exportGpxFile() {
  if (multiSpotPoints.length === 0) return;

  try {
    const res = await fetch('/api/gpx/export', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        points: multiSpotPoints,
        name: 'Bloom Flower Route',
      }),
    });
    if (res.ok) {
      const blob = await res.blob();
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = 'route.gpx';
      a.click();
      URL.revokeObjectURL(url);
    }
  } catch (err) {
    alert('匯出失敗: ' + err.message);
  }
}

// --- Favorites ---
async function openFavoritesModal() {
  el('favoritesModal').classList.remove('hidden');
  await renderFavoritesList();
}

function closeFavoritesModal() {
  el('favoritesModal').classList.add('hidden');
}

async function renderFavoritesList() {
  try {
    const res = await fetch('/api/favorites');
    if (res.ok) {
      const list = await res.json();
      const container = el('favoritesList');
      container.innerHTML = '';

      if (list.length === 0) {
        container.innerHTML = '<div class="empty-list-hint">目前無儲存的最愛地點。可以在傳送或路線中點擊「收藏」加入！</div>';
        return;
      }

      list.forEach(item => {
        const div = document.createElement('div');
        div.className = 'favorite-item';
        div.innerHTML = `
          <div class="fav-info">
            <span class="fav-name">${item.name}</span>
            <span class="fav-sub">${item.points.length} 個點位 · ${item.points[0] ? `${item.points[0].lat.toFixed(4)}, ${item.points[0].lng.toFixed(4)}` : ''}</span>
          </div>
          <div class="fav-actions">
            <button class="btn btn-sm btn-primary" onclick="loadFavorite('${item.id}')">載入</button>
            <button class="btn btn-sm btn-outline text-danger" onclick="deleteFavorite('${item.id}')">&times;</button>
          </div>
        `;
        container.appendChild(div);
      });
    }
  } catch (e) {
    console.error('Favorites list error:', e);
  }
}

window.loadFavorite = async function(id) {
  try {
    const res = await fetch('/api/favorites');
    if (res.ok) {
      const list = await res.json();
      const item = list.find(f => f.id === id);
      if (item) {
        closeFavoritesModal();
        if (item.points.length === 1) {
          switchTab('teleport');
          setTeleportTarget(item.points[0].lat, item.points[0].lng);
          map.flyTo([item.points[0].lat, item.points[0].lng], 16);
        } else {
          switchTab('multispot');
          multiSpotLeadIn = null;
          multiSpotPoints = item.points;
          renderMultiSpotList();
          await planMultiSpotRoute();
          map.fitBounds(L.latLngBounds(item.points.map(p => [p.lat, p.lng])));
        }
      }
    }
  } catch (e) {
    console.error('Load favorite error:', e);
  }
};

window.deleteFavorite = async function(id) {
  if (!confirm('確定刪除此最愛項目？')) return;
  try {
    await fetch(`/api/favorites?id=${id}`, { method: 'DELETE' });
    await renderFavoritesList();
  } catch (e) {
    console.error('Delete favorite error:', e);
  }
};

async function saveCurrentToFavorites(type) {
  let name = '';
  let points = [];

  if (type === 'teleport') {
    const lat = parseFloat(el('teleportLat').value);
    const lng = parseFloat(el('teleportLng').value);
    if (isNaN(lat) || isNaN(lng)) {
      alert('請先在傳送中選擇目標點');
      return;
    }
    name = prompt('請輸入最愛地點名稱：', `目標點 (${lat.toFixed(3)}, ${lng.toFixed(3)})`);
    points = [{ lat, lng }];
  } else if (type === 'route') {
    if (multiSpotPoints.length === 0) {
      alert('目前無路線點位可儲存');
      return;
    }
    name = prompt('請輸入最愛路線名稱：', '巡迴種花路線');
    points = multiSpotPoints;
  }

  if (!name || name.trim() === '') return;

  try {
    const res = await fetch('/api/favorites', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: name.trim(), points }),
    });
    if (res.ok) {
      alert('⭐ 已成功加入我的最愛！');
    }
  } catch (e) {
    alert('儲存失敗: ' + e.message);
  }
}

// --- Tab Navigation ---
function switchTab(tabId) {
  activeTab = tabId;
  document.querySelectorAll('.tab-btn').forEach(btn => {
    btn.classList.toggle('active', btn.dataset.tab === tabId);
  });
  document.querySelectorAll('.tab-content').forEach(content => {
    content.classList.toggle('active', content.id === `tab-${tabId}`);
  });

  if (tabId === 'twospot') {
    clearMultiSpotPoints();
  } else if (tabId === 'multispot') {
    clearTwoSpotRoute();
  }
}

// --- Event Listeners Setup ---
function initEventListeners() {
  // Tabs
  document.querySelectorAll('.tab-btn').forEach(btn => {
    btn.addEventListener('click', () => switchTab(btn.dataset.tab));
  });

  // Top bar
  if (el('locateComputerBtn')) el('locateComputerBtn').addEventListener('click', locateComputerPosition);
  if (el('locateRealBtn')) el('locateRealBtn').addEventListener('click', () => locateRealPosition(true, false));
  if (el('refreshDevicesBtn')) el('refreshDevicesBtn').addEventListener('click', () => loadDevices(false));
  if (el('toggleConnectBtn')) el('toggleConnectBtn').addEventListener('click', toggleConnect);
  if (el('restoreLocationBtn')) el('restoreLocationBtn').addEventListener('click', openSafeRestoreModal);
  if (el('onboardingTourBtn')) el('onboardingTourBtn').addEventListener('click', openOnboardingModal);
  if (el('safetyGuideBtn')) el('safetyGuideBtn').addEventListener('click', openSafetyGuideModal);
  if (el('favoritesBtn')) el('favoritesBtn').addEventListener('click', openFavoritesModal);
  if (el('searchInput')) el('searchInput').addEventListener('input', onSearchInput);
  document.addEventListener('click', (e) => {
    if (el('searchInput') && !el('searchInput').contains(e.target)) {
      el('searchResults').classList.add('hidden');
    }
  });

  // Mobile drawer toggles
  const toggleMobilePanel = () => {
    const panel = document.querySelector('.control-panel');
    if (panel) panel.classList.toggle('mobile-open');
  };
  if (el('mobilePanelToggleBtn')) el('mobilePanelToggleBtn').addEventListener('click', toggleMobilePanel);
  if (el('mobileFloatingPanelBtn')) el('mobileFloatingPanelBtn').addEventListener('click', toggleMobilePanel);

  if (el('profileSelect')) {
    el('profileSelect').addEventListener('change', async (e) => {
      try {
        const res = await fetch('/api/profile', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ id: e.target.value }),
        });
        if (res.ok) {
          const state = await res.json();
          updateAppState(state);
        }
      } catch (err) {
        console.error('Failed to change profile:', err);
      }
    });
  }

  // Onboarding Modal
  if (el('closeOnboardingModalBtn')) el('closeOnboardingModalBtn').addEventListener('click', closeOnboardingModal);
  if (el('skipOnboardingBtn')) el('skipOnboardingBtn').addEventListener('click', closeOnboardingModal);
  if (el('finishOnboardingBtn')) el('finishOnboardingBtn').addEventListener('click', closeOnboardingModal);
  if (el('nextOnboardingBtn')) el('nextOnboardingBtn').addEventListener('click', nextOnboardingStep);
  if (el('prevOnboardingBtn')) el('prevOnboardingBtn').addEventListener('click', prevOnboardingStep);
  document.querySelectorAll('.onboarding-dots .dot').forEach(dot => {
    dot.addEventListener('click', () => showOnboardingStep(parseInt(dot.dataset.step)));
  });

  // Speed slider
  el('speedSlider').addEventListener('input', (e) => {
    const val = parseFloat(e.target.value);
    el('currentSpeedDisplay').textContent = val.toFixed(1);
    checkSpeedLimit(val);
  });
  el('speedSlider').addEventListener('change', (e) => {
    setSpeed(parseFloat(e.target.value));
  });

  // Teleport
  el('execTeleportBtn').addEventListener('click', execTeleport);
  el('fillRealLocBtn').addEventListener('click', fillRealLocationToTeleport);
  el('addTeleportFavBtn').addEventListener('click', () => saveCurrentToFavorites('teleport'));

  // Two-spot
  if (el('clearTwoSpotBtn')) {
    el('clearTwoSpotBtn').addEventListener('click', clearTwoSpotRoute);
  }
  el('twoSpotSnapSelect').addEventListener('change', planTwoSpotRoute);
  el('twoSpotStartBtn').addEventListener('click', startTwoSpotRoute);
  el('twoSpotPauseBtn').addEventListener('click', async () => {
    const endpoint = (appState && appState.route && appState.route.paused) ? '/api/route/resume' : '/api/route/pause';
    const res = await fetch(endpoint, { method: 'POST' });
    if (res.ok) updateAppState(await res.json());
  });
  el('twoSpotStopBtn').addEventListener('click', async () => {
    const res = await fetch('/api/route/stop', { method: 'POST' });
    if (res.ok) updateAppState(await res.json());
  });

  // Multi-spot
  el('clearWaypointsBtn').addEventListener('click', clearMultiSpotPoints);
  el('multiSpotSnapSelect').addEventListener('change', planMultiSpotRoute);
  el('multiSpotStartBtn').addEventListener('click', startMultiSpotRoute);
  el('multiSpotPauseBtn').addEventListener('click', async () => {
    const endpoint = (appState && appState.route && appState.route.paused) ? '/api/route/resume' : '/api/route/pause';
    const res = await fetch(endpoint, { method: 'POST' });
    if (res.ok) updateAppState(await res.json());
  });
  el('multiSpotStopBtn').addEventListener('click', async () => {
    const res = await fetch('/api/route/stop', { method: 'POST' });
    if (res.ok) updateAppState(await res.json());
  });
  el('exportGpxBtn').addEventListener('click', exportGpxFile);
  el('importGpxInput').addEventListener('change', importGpxFile);
  el('saveMultiRouteFavBtn').addEventListener('click', () => saveCurrentToFavorites('route'));

  // Jump
  el('clearJumpPointsBtn').addEventListener('click', clearJumpPoints);
  el('jumpNextBtn').addEventListener('click', () => {
    autoJumpActive = el('autoJumpCheck').checked;
    triggerNextJump();
  });
  el('jumpStopBtn').addEventListener('click', () => {
    autoJumpActive = false;
    renderJumpList();
  });

  // Flower Loop Modal
  el('openFlowerLoopModalBtn').addEventListener('click', openFlowerLoopModal);
  el('closeFlowerLoopModalBtn').addEventListener('click', closeFlowerLoopModal);
  el('cancelFlowerLoopBtn').addEventListener('click', closeFlowerLoopModal);
  el('applyFlowerLoopBtn').addEventListener('click', applyFlowerLoop);
  el('flowerRadiusInput').addEventListener('input', updateFlowerLoopCalculator);
  document.querySelectorAll('.radius-preset-btn').forEach(btn => {
    btn.addEventListener('click', (e) => {
      document.querySelectorAll('.radius-preset-btn').forEach(b => b.classList.remove('active'));
      btn.classList.add('active');
      el('flowerRadiusInput').value = btn.dataset.radius;
      updateFlowerLoopCalculator();
    });
  });

  // Safe Restore Modal
  el('closeSafeRestoreModalBtn').addEventListener('click', closeSafeRestoreModal);
  el('cancelRestoreBtn').addEventListener('click', closeSafeRestoreModal);
  el('confirmRestoreBtn').addEventListener('click', executeSafeRestore);
  el('confirmGameClosedCheckbox').addEventListener('change', (e) => {
    el('confirmRestoreBtn').disabled = !e.target.checked;
  });

  // Safety Guide Modal
  el('closeSafetyGuideModalBtn').addEventListener('click', closeSafetyGuideModal);
  el('closeSafetyGuideBtn').addEventListener('click', closeSafetyGuideModal);
  document.querySelectorAll('.guide-tab-btn').forEach(btn => {
    btn.addEventListener('click', () => switchGuideTab(btn.dataset.guidetab));
  });

  // Favorites Modal
  el('closeFavoritesModalBtn').addEventListener('click', closeFavoritesModal);

  // Joystick
  initJoystick();
}

async function loadState() {
  try {
    const res = await fetch('/api/state');
    if (res.ok) {
      const state = await res.json();
      updateAppState(state);
    }
  } catch (e) {
    console.error('Failed to load initial state:', e);
  }
}
