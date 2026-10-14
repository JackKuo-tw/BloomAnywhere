//! Spherical-earth geodesy helpers (mean radius 6_371_008.8 m).
use crate::LatLng;

pub const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// Normalise a longitude in degrees to [-180, 180].
pub fn normalize_lng(lng: f64) -> f64 {
    if !lng.is_finite() {
        return lng;
    }
    if (-180.0..=180.0).contains(&lng) {
        return lng;
    }
    let r = (lng + 180.0).rem_euclid(360.0) - 180.0;
    // rem_euclid maps 180 to -180; keep it in range either way.
    r.clamp(-180.0, 180.0)
}

/// Normalise a bearing in degrees to [0, 360).
pub fn normalize_bearing(deg: f64) -> f64 {
    if !deg.is_finite() {
        return 0.0;
    }
    let r = deg.rem_euclid(360.0);
    // rem_euclid can return exactly 360.0 for tiny negative inputs due to rounding.
    if r >= 360.0 {
        0.0
    } else {
        r
    }
}

/// Great-circle distance in metres.
pub fn haversine_m(a: LatLng, b: LatLng) -> f64 {
    let (lat1, lat2) = (a.lat.to_radians(), b.lat.to_radians());
    let dlat = lat2 - lat1;
    let dlng = (b.lng - a.lng).to_radians();
    let s1 = (dlat / 2.0).sin();
    let s2 = (dlng / 2.0).sin();
    let h = (s1 * s1 + lat1.cos() * lat2.cos() * s2 * s2).clamp(0.0, 1.0);
    2.0 * EARTH_RADIUS_M * h.sqrt().asin()
}

/// Initial bearing from `a` to `b`, degrees clockwise from north in [0, 360).
pub fn bearing_deg(a: LatLng, b: LatLng) -> f64 {
    let (lat1, lat2) = (a.lat.to_radians(), b.lat.to_radians());
    let dlng = (b.lng - a.lng).to_radians();
    let y = dlng.sin() * lat2.cos();
    let x = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * dlng.cos();
    if y == 0.0 && x == 0.0 {
        return 0.0;
    }
    normalize_bearing(y.atan2(x).to_degrees())
}

/// Point reached travelling `dist_m` metres from `a` on initial bearing `bearing_deg`.
pub fn destination(a: LatLng, bearing_deg: f64, dist_m: f64) -> LatLng {
    if dist_m == 0.0 || !dist_m.is_finite() || !bearing_deg.is_finite() {
        return a;
    }
    let delta = dist_m / EARTH_RADIUS_M;
    let theta = bearing_deg.to_radians();
    let lat1 = a.lat.to_radians();
    let lng1 = a.lng.to_radians();
    let sin_lat2 =
        (lat1.sin() * delta.cos() + lat1.cos() * delta.sin() * theta.cos()).clamp(-1.0, 1.0);
    let lat2 = sin_lat2.asin();
    let y = theta.sin() * delta.sin() * lat1.cos();
    let x = delta.cos() - lat1.sin() * sin_lat2;
    let lng2 = lng1 + y.atan2(x);
    LatLng::new(lat2.to_degrees(), normalize_lng(lng2.to_degrees()))
}

/// Linear (good enough for < few km) interpolation, t in [0,1].
///
/// `t` is clamped to [0,1]; the shorter way across the antimeridian is used.
pub fn interpolate(a: LatLng, b: LatLng, t: f64) -> LatLng {
    let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
    let mut dlng = b.lng - a.lng;
    if dlng > 180.0 {
        dlng -= 360.0;
    } else if dlng < -180.0 {
        dlng += 360.0;
    }
    LatLng::new(a.lat + (b.lat - a.lat) * t, normalize_lng(a.lng + dlng * t))
}

/// Sum of segment lengths in metres.
pub fn path_length_m(path: &[LatLng]) -> f64 {
    path.windows(2).map(|w| haversine_m(w[0], w[1])).sum()
}

/// Cumulative distances (metres) of each vertex from the start; same length as `path`.
pub fn cumulative_m(path: &[LatLng]) -> Vec<f64> {
    let mut cum = Vec::with_capacity(path.len());
    let mut acc = 0.0;
    for (i, p) in path.iter().enumerate() {
        if i > 0 {
            acc += haversine_m(path[i - 1], *p);
        }
        cum.push(acc);
    }
    cum
}

/// Locate `dist_m` along `path` given precomputed cumulative distances (see [`cumulative_m`]).
/// Zero-length segments are skipped; result is clamped to the ends.
pub(crate) fn locate(path: &[LatLng], cum: &[f64], dist_m: f64) -> (LatLng, usize, f64) {
    let n = path.len();
    match n {
        0 => return (LatLng::new(0.0, 0.0), 0, 0.0),
        1 => return (path[0], 0, 0.0),
        _ => {}
    }
    let total = cum[n - 1];
    let d = if dist_m.is_nan() {
        0.0
    } else {
        dist_m.clamp(0.0, total)
    };
    // First vertex whose cumulative distance is strictly greater than d.
    let idx = cum.partition_point(|&c| c <= d);
    if idx >= n || idx == 0 {
        // At (or beyond) the end: use the last non-degenerate segment for the bearing.
        let i = (0..n - 1)
            .rev()
            .find(|&i| cum[i + 1] > cum[i])
            .unwrap_or(n - 2);
        let at = if idx == 0 { path[0] } else { path[n - 1] };
        return (at, i, bearing_deg(path[i], path[i + 1]));
    }
    let i = idx - 1; // cum[i] <= d < cum[i+1] => segment i has non-zero length
    let (a, b) = (path[i], path[i + 1]);
    let brg = bearing_deg(a, b);
    let off = d - cum[i];
    let p = if off <= 0.0 {
        a
    } else {
        destination(a, brg, off)
    };
    (p, i, brg)
}

/// Position at `dist_m` along polyline (clamped to ends). Returns (point, index of segment start, bearing of that segment).
pub fn point_along(path: &[LatLng], dist_m: f64) -> (LatLng, usize, f64) {
    let cum = cumulative_m(path);
    locate(path, &cum, dist_m)
}

/// Closed polygon approximating a circle of `radius_m` around `center`, `n` points (first != last).
pub fn circle(center: LatLng, radius_m: f64, n: usize) -> Vec<LatLng> {
    (0..n)
        .map(|i| destination(center, 360.0 * i as f64 / n as f64, radius_m))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAIPEI_101: LatLng = LatLng::new(25.033964, 121.564468);
    const TAIPEI_MAIN: LatLng = LatLng::new(25.047675, 121.517055);

    #[test]
    fn known_distance_taipei() {
        let d = haversine_m(TAIPEI_101, TAIPEI_MAIN);
        assert!((d - 5000.0).abs() / 5000.0 < 0.01, "got {d}");
        assert!((haversine_m(TAIPEI_MAIN, TAIPEI_101) - d).abs() < 1e-6);
    }

    #[test]
    fn one_degree_latitude() {
        let d = haversine_m(LatLng::new(0.0, 0.0), LatLng::new(1.0, 0.0));
        assert!((d - 111_195.0).abs() < 5.0, "got {d}");
        assert_eq!(haversine_m(TAIPEI_101, TAIPEI_101), 0.0);
    }

    #[test]
    fn bearing_quadrants() {
        let o = LatLng::new(0.0, 0.0);
        let cases = [
            (LatLng::new(1.0, 0.0), 0.0),
            (LatLng::new(0.0, 1.0), 90.0),
            (LatLng::new(-1.0, 0.0), 180.0),
            (LatLng::new(0.0, -1.0), 270.0),
            (LatLng::new(1.0, 1.0), 45.0),
            (LatLng::new(-1.0, 1.0), 135.0),
            (LatLng::new(-1.0, -1.0), 225.0),
            (LatLng::new(1.0, -1.0), 315.0),
        ];
        for (p, want) in cases {
            let b = bearing_deg(o, p);
            assert!((0.0..360.0).contains(&b));
            assert!((b - want).abs() < 0.1, "to {p:?}: got {b}, want {want}");
        }
        assert_eq!(bearing_deg(o, o), 0.0);
        // Taipei Main Station is west-north-west of Taipei 101.
        let b = bearing_deg(TAIPEI_101, TAIPEI_MAIN);
        assert!((280.0..300.0).contains(&b), "got {b}");
    }

    #[test]
    fn destination_roundtrip() {
        for &brg in &[0.0, 37.0, 90.0, 145.5, 180.0, 250.0, 359.0] {
            for &d in &[1.0, 100.0, 5_000.0, 250_000.0] {
                let p = destination(TAIPEI_101, brg, d);
                assert!(p.is_valid());
                let back = haversine_m(TAIPEI_101, p);
                assert!(
                    (back - d).abs() < 1e-6 * d.max(1.0),
                    "brg {brg} d {d} got {back}"
                );
                let b = bearing_deg(TAIPEI_101, p);
                let diff = ((b - brg + 540.0) % 360.0 - 180.0).abs();
                assert!(diff < 1e-6, "brg {brg} got {b}");
            }
        }
        let p = destination(
            TAIPEI_101,
            bearing_deg(TAIPEI_101, TAIPEI_MAIN),
            haversine_m(TAIPEI_101, TAIPEI_MAIN),
        );
        assert!(haversine_m(p, TAIPEI_MAIN) < 0.01);
        assert_eq!(destination(TAIPEI_101, 10.0, 0.0), TAIPEI_101);
        assert_eq!(destination(TAIPEI_101, f64::NAN, 10.0), TAIPEI_101);
    }

    #[test]
    fn destination_antimeridian() {
        let p = destination(LatLng::new(0.0, 179.9999), 90.0, 1000.0);
        assert!(p.is_valid(), "{p:?}");
        assert!(p.lng < -179.0);
    }

    #[test]
    fn interpolate_basic() {
        let a = LatLng::new(0.0, 0.0);
        let b = LatLng::new(2.0, 4.0);
        assert_eq!(interpolate(a, b, 0.5), LatLng::new(1.0, 2.0));
        assert_eq!(interpolate(a, b, 0.0), a);
        assert_eq!(interpolate(a, b, 1.0), b);
        assert_eq!(interpolate(a, b, 2.0), b);
        let m = interpolate(LatLng::new(0.0, 179.0), LatLng::new(0.0, -179.0), 0.5);
        assert!((m.lng.abs() - 180.0).abs() < 1e-9, "{m:?}");
    }

    #[test]
    fn path_length_and_point_along() {
        let path = [TAIPEI_101, TAIPEI_101, TAIPEI_MAIN, TAIPEI_MAIN];
        let len = path_length_m(&path);
        assert!((len - haversine_m(TAIPEI_101, TAIPEI_MAIN)).abs() < 1e-9);

        let (p, i, b) = point_along(&path, len / 2.0);
        assert_eq!(i, 1);
        assert!((haversine_m(TAIPEI_101, p) - len / 2.0).abs() < 1e-3);
        assert!((b - bearing_deg(TAIPEI_101, TAIPEI_MAIN)).abs() < 1e-9);

        let (p, i, _) = point_along(&path, -10.0);
        assert_eq!(p, TAIPEI_101);
        assert_eq!(i, 1);
        let (p, i, b) = point_along(&path, len * 10.0);
        assert_eq!(p, TAIPEI_MAIN);
        assert_eq!(i, 1);
        assert!(b.is_finite());
        let (p, _, _) = point_along(&path, f64::NAN);
        assert_eq!(p, TAIPEI_101);
    }

    #[test]
    fn point_along_degenerate() {
        assert_eq!(path_length_m(&[]), 0.0);
        assert_eq!(path_length_m(&[TAIPEI_101]), 0.0);
        let (p, i, b) = point_along(&[], 5.0);
        assert!(p.lat.is_finite() && b.is_finite());
        assert_eq!(i, 0);
        assert_eq!(point_along(&[TAIPEI_101], 5.0), (TAIPEI_101, 0, 0.0));
        let (p, _, b) = point_along(&[TAIPEI_101, TAIPEI_101, TAIPEI_101], 5.0);
        assert_eq!(p, TAIPEI_101);
        assert_eq!(b, 0.0);
    }

    #[test]
    fn circle_points() {
        let c = circle(TAIPEI_101, 500.0, 36);
        assert_eq!(c.len(), 36);
        assert_ne!(c.first(), c.last());
        for p in &c {
            assert!((haversine_m(TAIPEI_101, *p) - 500.0).abs() < 1e-6);
        }
        assert!(circle(TAIPEI_101, 500.0, 0).is_empty());
        assert_eq!(circle(TAIPEI_101, 500.0, 1).len(), 1);
    }

    #[test]
    fn normalisation() {
        assert_eq!(normalize_lng(190.0), -170.0);
        assert_eq!(normalize_lng(-190.0), 170.0);
        assert_eq!(normalize_lng(180.0), 180.0);
        assert_eq!(normalize_bearing(-90.0), 270.0);
        assert_eq!(normalize_bearing(720.0), 0.0);
        assert_eq!(normalize_bearing(-1e-20), 0.0);
    }
}
