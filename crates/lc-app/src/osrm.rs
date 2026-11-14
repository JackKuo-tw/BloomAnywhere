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
    #[serde(default)]
    legs: Vec<OsrmLeg>,
}

#[derive(Deserialize)]
struct OsrmLeg {
    #[serde(default)]
    steps: Vec<OsrmStep>,
}

#[derive(Deserialize)]
struct OsrmStep {
    geometry: Option<OsrmGeometry>,
}

#[derive(Deserialize)]
struct OsrmGeometry {
    coordinates: Vec<[f64; 2]>,
}

const MAX_OSRM_WAYPOINTS_PER_REQ: usize = 50;
const OFF_ROAD_WAYPOINT_THRESHOLD_M: f64 = 25.0;

fn push_deduped(out: &mut Vec<LatLng>, pt: LatLng) {
    if out
        .last()
        .map(|prev| haversine_m(*prev, pt) > 0.05)
        .unwrap_or(true)
    {
        out.push(pt);
    }
}

fn dedup_consecutive(points: Vec<LatLng>) -> Vec<LatLng> {
    let mut out: Vec<LatLng> = Vec::with_capacity(points.len());
    for pt in points {
        push_deduped(&mut out, pt);
    }
    out
}

fn process_leg(
    leg_start: LatLng,
    leg_end: LatLng,
    raw_leg: Vec<LatLng>,
    profile: &str,
) -> Vec<LatLng> {
    let deduped = dedup_consecutive(raw_leg);
    if deduped.len() < 2 {
        return vec![leg_start, leg_end];
    }

    let mut path = sanitize_route_loops(deduped);
    let distance_m = path_length_m(&path);

    // If pedestrian/bike route takes an absurd detour (> 3x straight dist)
    // for short distances (< 120m), fallback to direct line for this leg.
    if profile != "car" && profile != "driving" {
        let straight_dist = haversine_m(leg_start, leg_end);
        if straight_dist < 120.0 && distance_m > straight_dist * 3.0 && distance_m > 250.0 {
            return vec![leg_start, leg_end];
        }

        // Preserve off-road waypoints (e.g. inside a park/plaza > 25m from road)
        // so the planned route visibly visits the user's clicked point.
        if haversine_m(leg_start, path[0]) > OFF_ROAD_WAYPOINT_THRESHOLD_M {
            path.insert(0, leg_start);
        }
        if let Some(&last) = path.last() {
            if haversine_m(last, leg_end) > OFF_ROAD_WAYPOINT_THRESHOLD_M {
                path.push(leg_end);
            }
        }
    }

    path
}

fn process_osrm_route(
    points: &[LatLng],
    profile: &str,
    route: OsrmRoute,
) -> Option<RoutePlanResponse> {
    if points.len() >= 2 && route.legs.len() == points.len() - 1 {
        let mut combined: Vec<LatLng> = Vec::new();
        for (i, leg) in route.legs.into_iter().enumerate() {
            let raw_leg: Vec<LatLng> = leg
                .steps
                .into_iter()
                .filter_map(|s| s.geometry)
                .flat_map(|g| g.coordinates)
                .map(|c| LatLng::new(c[1], c[0]))
                .collect();

            let leg_path = if raw_leg.is_empty() {
                vec![points[i], points[i + 1]]
            } else {
                process_leg(points[i], points[i + 1], raw_leg, profile)
            };

            for pt in leg_path {
                push_deduped(&mut combined, pt);
            }
        }

        if combined.len() >= 2 {
            let distance_m = path_length_m(&combined);
            return Some(RoutePlanResponse {
                path: combined,
                distance_m,
            });
        }
    }

    let geom = route.geometry?;
    let raw_path: Vec<LatLng> = geom
        .coordinates
        .into_iter()
        .map(|c| LatLng::new(c[1], c[0]))
        .collect();

    let path = if points.len() == 2 {
        process_leg(points[0], points[1], raw_path, profile)
    } else {
        // Never run sanitize_route_loops across multi-point waypoints without leg boundaries
        dedup_consecutive(raw_path)
    };

    if path.len() < 2 {
        return None;
    }

    let distance_m = path_length_m(&path);
    Some(RoutePlanResponse { path, distance_m })
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

async fn plan_route_chunk(
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

    // steps=true provides per-leg geometry so loop sanitization stays scoped to
    // each leg and never deletes intermediate user waypoints; continue_straight=false
    // allows U-turns at intersections, preventing forced loops around blocks.
    let url = format!(
        "https://router.project-osrm.org/route/v1/{}/{}?overview=full&steps=true&geometries=geojson&continue_straight=false",
        osrm_mode, coords
    );

    match client
        .get(&url)
        .timeout(Duration::from_secs(5))
        .header("User-Agent", "BloomAnywhere/0.1.1")
        .send()
        .await
    {
        Ok(res) if res.status().is_success() => {
            if let Ok(osrm) = res.json::<OsrmResponse>().await {
                if osrm.code == "Ok" {
                    if let Some(routes) = osrm.routes {
                        if let Some(first) = routes.into_iter().next() {
                            if let Some(resp) = process_osrm_route(points, profile, first) {
                                return resp;
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

pub async fn plan_route(
    client: &reqwest::Client,
    points: &[LatLng],
    profile: &str,
) -> RoutePlanResponse {
    if points.len() <= MAX_OSRM_WAYPOINTS_PER_REQ || profile == "straight" {
        return plan_route_chunk(client, points, profile).await;
    }

    let mut full_path: Vec<LatLng> = Vec::new();
    let mut start = 0;
    while start < points.len() - 1 {
        let end = (start + MAX_OSRM_WAYPOINTS_PER_REQ).min(points.len());
        let chunk = &points[start..end];
        let resp = plan_route_chunk(client, chunk, profile).await;
        for pt in resp.path {
            push_deduped(&mut full_path, pt);
        }
        if end == points.len() {
            break;
        }
        start = end - 1;
    }

    let distance_m = path_length_m(&full_path);
    RoutePlanResponse {
        path: full_path,
        distance_m,
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

    #[test]
    fn test_multipoint_route_does_not_skip_intermediate_waypoints() {
        // Waypoints 1 -> 2 -> 3 -> 4 forming a loop around a block where 4 is near 1.
        // Previously sanitize_route_loops ran across the combined path and deleted 2 and 3.
        let p1 = LatLng::new(25.03300, 121.56400);
        let p2 = LatLng::new(25.03400, 121.56400); // ~111m N
        let p3 = LatLng::new(25.03400, 121.56500); // ~100m E
        let p4 = LatLng::new(25.03305, 121.56410); // ~11m from p1!

        let step_coords = |pts: &[LatLng]| OsrmStep {
            geometry: Some(OsrmGeometry {
                coordinates: pts.iter().map(|p| [p.lng, p.lat]).collect(),
            }),
        };

        let route = OsrmRoute {
            geometry: Some(OsrmGeometry {
                coordinates: vec![p1, p2, p3, p4]
                    .into_iter()
                    .map(|p| [p.lng, p.lat])
                    .collect(),
            }),
            legs: vec![
                OsrmLeg {
                    steps: vec![step_coords(&[p1, p2])],
                },
                OsrmLeg {
                    steps: vec![step_coords(&[p2, p3])],
                },
                OsrmLeg {
                    steps: vec![step_coords(&[p3, p4])],
                },
            ],
        };

        let resp = process_osrm_route(&[p1, p2, p3, p4], "foot", route).expect("route response");
        assert!(resp.path.iter().any(|p| haversine_m(*p, p2) < 1.0));
        assert!(resp.path.iter().any(|p| haversine_m(*p, p3) < 1.0));
        assert_eq!(resp.path.len(), 4);
    }

    #[test]
    fn test_multipoint_preserves_offroad_waypoints_and_short_detour_fallback() {
        let p1 = LatLng::new(25.03390, 121.56440);
        let p2_offroad = LatLng::new(25.03420, 121.56500); // >25m off road
        let p2_road = LatLng::new(25.03420, 121.56540); // ~40m east of p2_offroad
        let p3 = LatLng::new(25.03360, 121.56540);
        let detour_mid = LatLng::new(25.03550, 121.56440); // 360m detour for 67m leg

        let step_coords = |pts: &[LatLng]| OsrmStep {
            geometry: Some(OsrmGeometry {
                coordinates: pts.iter().map(|p| [p.lng, p.lat]).collect(),
            }),
        };

        let route = OsrmRoute {
            geometry: None,
            legs: vec![
                // Leg 0: absurd car one-way detour for short <120m walk -> falls back to direct [p1, p2_offroad]
                OsrmLeg {
                    steps: vec![step_coords(&[p1, detour_mid, p2_road])],
                },
                // Leg 1: normal road segment starting >25m away from p2_offroad -> keeps p2_offroad
                OsrmLeg {
                    steps: vec![step_coords(&[p2_road, p3])],
                },
            ],
        };

        let resp =
            process_osrm_route(&[p1, p2_offroad, p3], "foot", route).expect("route response");
        assert!(resp.path.iter().any(|p| haversine_m(*p, p1) < 1.0));
        assert!(resp.path.iter().any(|p| haversine_m(*p, p2_offroad) < 1.0));
        assert!(resp.path.iter().any(|p| haversine_m(*p, p3) < 1.0));
        assert!(!resp.path.iter().any(|p| haversine_m(*p, detour_mid) < 1.0));
    }
}
