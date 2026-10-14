use lc_core::geo::{haversine_m, path_length_m};
use lc_core::LatLng;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoutePlanRequest {
    pub points: Vec<LatLng>,
    #[serde(default = "default_profile")]
    pub profile: String,
}

fn default_profile() -> String {
    "foot".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoutePlanResponse {
    pub path: Vec<LatLng>,
    pub distance_m: f64,
}

#[derive(Deserialize)]
struct OsrmResponse {
    code: String,
    routes: Option<Vec<OsrmRoute>>,
}

#[derive(Deserialize)]
struct OsrmRoute {
    geometry: Option<OsrmGeometry>,
}

#[derive(Deserialize)]
struct OsrmGeometry {
    coordinates: Vec<[f64; 2]>,
}

/// Detects and eliminates redundant 360-degree detour loops around city blocks.
pub fn sanitize_route_loops(mut path: Vec<LatLng>) -> Vec<LatLng> {
    if path.len() < 4 {
        return path;
    }

    // 1. Detect start loop: path leaves p[0], walks around the block, and returns near p[0]
    let n = path.len();
    let p0 = path[0];
    let mut best_start_cut = None;

    let limit = (n / 2).clamp(4, 30).min(n - 1);
    for k in 3..limit {
        let dist_to_start = haversine_m(p0, path[k]);
        let path_len_so_far = path_length_m(&path[0..=k]);
        // If we returned to within 35 meters of start, but we walked > 50 meters,
        // and path length is > 2.2x the straight distance to p[k]
        if dist_to_start <= 35.0 && path_len_so_far >= 50.0 && path_len_so_far > dist_to_start * 2.2
        {
            best_start_cut = Some(k);
        }
    }

    if let Some(cut_idx) = best_start_cut {
        let mut new_path = vec![p0];
        new_path.extend_from_slice(&path[cut_idx..]);
        path = new_path;
    }

    // 2. Check for intermediate redundant loops
    let mut i = 0;
    while i + 3 < path.len() {
        let mut shortcut = None;
        let max_j = (i + 15).min(path.len());
        for j in (i + 3)..max_j {
            let dist_ij = haversine_m(path[i], path[j]);
            let loop_len = path_length_m(&path[i..=j]);
            if dist_ij <= 25.0 && loop_len >= 50.0 && loop_len > dist_ij * 2.5 {
                shortcut = Some(j);
            }
        }
        if let Some(j) = shortcut {
            path.drain((i + 1)..j);
        } else {
            i += 1;
        }
    }

    path
}

pub async fn plan_route(
    client: &reqwest::Client,
    points: &[LatLng],
    profile: &str,
) -> RoutePlanResponse {
    if points.len() <= 1 || profile == "straight" {
        return RoutePlanResponse {
            path: points.to_vec(),
            distance_m: path_length_m(points),
        };
    }

    let osrm_mode = match profile {
        "bike" | "cycling" => "cycling",
        "car" | "driving" => "driving",
        _ => "walking",
    };

    let coord_str: Vec<String> = points
        .iter()
        .map(|p| format!("{:.6},{:.6}", p.lng, p.lat))
        .collect();
    let coords = coord_str.join(";");

    // continue_straight=false allows U-turns at intersections, preventing forced loops around blocks
    let url = format!(
        "https://router.project-osrm.org/route/v1/{}/{}?overview=full&geometries=geojson&continue_straight=false",
        osrm_mode, coords
    );

    match client
        .get(&url)
        .timeout(Duration::from_secs(5))
        .header("User-Agent", "BloomAnywhere/0.1.0")
        .send()
        .await
    {
        Ok(res) if res.status().is_success() => {
            if let Ok(osrm) = res.json::<OsrmResponse>().await {
                if osrm.code == "Ok" {
                    if let Some(routes) = osrm.routes {
                        if let Some(first) = routes.into_iter().next() {
                            if let Some(geom) = first.geometry {
                                let raw_path: Vec<LatLng> = geom
                                    .coordinates
                                    .into_iter()
                                    .map(|c| LatLng::new(c[1], c[0]))
                                    .collect();

                                // Sanitize any 360-degree block loops
                                let path = sanitize_route_loops(raw_path);
                                let distance_m = path_length_m(&path);

                                // If 2 points and pedestrian route takes an absurd detour (> 3x straight dist)
                                // for short distances (< 120m), fallback to direct line
                                if points.len() == 2 && profile != "car" {
                                    let straight_dist = haversine_m(points[0], points[1]);
                                    if straight_dist < 120.0
                                        && distance_m > straight_dist * 3.0
                                        && distance_m > 250.0
                                    {
                                        return RoutePlanResponse {
                                            path: points.to_vec(),
                                            distance_m: straight_dist,
                                        };
                                    }
                                }

                                return RoutePlanResponse { path, distance_m };
                            }
                        }
                    }
                }
            }
        }
        Err(e) => {
            tracing::warn!("OSRM routing request failed: {e:#}, falling back to straight line");
        }
        _ => {
            tracing::warn!("OSRM routing returned non-success, falling back to straight line");
        }
    }

    // Fallback to straight line
    RoutePlanResponse {
        path: points.to_vec(),
        distance_m: path_length_m(points),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_start_loop() {
        let p0 = LatLng::new(25.03300, 121.56400);
        // Loop around block: North -> East -> South -> back near start
        let p1 = LatLng::new(25.03400, 121.56400); // 111m north
        let p2 = LatLng::new(25.03400, 121.56500); // 100m east
        let p3 = LatLng::new(25.03300, 121.56500); // 111m south
        let p4 = LatLng::new(25.03305, 121.56410); // ~11m from p0, after ~330m loop!
        let p5 = LatLng::new(25.03200, 121.56410); // outbound towards destination
        let p6 = LatLng::new(25.03100, 121.56410);

        let looped_path = vec![p0, p1, p2, p3, p4, p5, p6];
        let cleaned = sanitize_route_loops(looped_path);

        // Should have pruned p1, p2, p3
        assert_eq!(cleaned[0], p0);
        assert_eq!(cleaned[1], p4);
        assert_eq!(cleaned[2], p5);
        assert_eq!(cleaned[3], p6);
        assert_eq!(cleaned.len(), 4);
    }

    #[test]
    fn test_sanitize_clean_path_unchanged() {
        let p0 = LatLng::new(25.03300, 121.56400);
        let p1 = LatLng::new(25.03350, 121.56450);
        let p2 = LatLng::new(25.03400, 121.56500);
        let normal_path = vec![p0, p1, p2];

        let result = sanitize_route_loops(normal_path.clone());
        assert_eq!(result, normal_path);
    }
}
