use lc_core::LatLng;

pub fn parse_gpx_content(xml: &str) -> anyhow::Result<Vec<LatLng>> {
    if let Ok(Ok(points)) = std::panic::catch_unwind(|| lc_core::gpx::parse_gpx(xml)) {
        if !points.is_empty() {
            return Ok(points);
        }
    }

    // Fallback parser: scan for trkpt, rtept, or wpt tags
    let mut trkpts = Vec::new();
    let mut rtepts = Vec::new();
    let mut wpts = Vec::new();

    for line in xml.lines() {
        if line.contains("<trkpt") {
            if let Some(pt) = parse_lat_lon_tag(line) {
                trkpts.push(pt);
            }
        } else if line.contains("<rtept") {
            if let Some(pt) = parse_lat_lon_tag(line) {
                rtepts.push(pt);
            }
        } else if line.contains("<wpt") {
            if let Some(pt) = parse_lat_lon_tag(line) {
                wpts.push(pt);
            }
        }
    }

    if !trkpts.is_empty() {
        return Ok(trkpts);
    }
    if !rtepts.is_empty() {
        return Ok(rtepts);
    }
    if !wpts.is_empty() {
        return Ok(wpts);
    }

    // Try multi-line search if not on a single line
    let mut all_found = Vec::new();
    let re_lat = |s: &str| -> Option<f64> {
        let idx = s.find("lat=\"")?;
        let rest = &s[idx + 5..];
        let end = rest.find('"')?;
        rest[..end].parse().ok()
    };
    let re_lon = |s: &str| -> Option<f64> {
        let idx = s.find("lon=\"")?;
        let rest = &s[idx + 5..];
        let end = rest.find('"')?;
        rest[..end].parse().ok()
    };

    let mut pos = 0;
    while let Some(tag_start) = xml[pos..].find('<') {
        let tag_pos = pos + tag_start;
        if let Some(tag_end) = xml[tag_pos..].find('>') {
            let full_tag = &xml[tag_pos..=tag_pos + tag_end];
            if full_tag.starts_with("<trkpt")
                || full_tag.starts_with("<rtept")
                || full_tag.starts_with("<wpt")
            {
                if let (Some(lat), Some(lng)) = (re_lat(full_tag), re_lon(full_tag)) {
                    all_found.push(LatLng::new(lat, lng));
                }
            }
            pos = tag_pos + tag_end + 1;
        } else {
            break;
        }
    }

    if !all_found.is_empty() {
        return Ok(all_found);
    }

    anyhow::bail!("No waypoints, track points, or route points found in GPX")
}

fn parse_lat_lon_tag(tag: &str) -> Option<LatLng> {
    let lat_idx = tag.find("lat=\"")?;
    let rest_lat = &tag[lat_idx + 5..];
    let lat_end = rest_lat.find('"')?;
    let lat: f64 = rest_lat[..lat_end].parse().ok()?;

    let lon_idx = tag.find("lon=\"")?;
    let rest_lon = &tag[lon_idx + 5..];
    let lon_end = rest_lon.find('"')?;
    let lng: f64 = rest_lon[..lon_end].parse().ok()?;

    let pt = LatLng::new(lat, lng);
    if pt.is_valid() {
        Some(pt)
    } else {
        None
    }
}

pub fn to_gpx_content(name: &str, points: &[LatLng]) -> String {
    if let Ok(s) = std::panic::catch_unwind(|| lc_core::gpx::to_gpx(name, points)) {
        if !s.is_empty() {
            return s;
        }
    }

    let escaped_name = name
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");

    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<gpx version=\"1.1\" creator=\"BloomAnywhere\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n");
    xml.push_str(&format!(
        "  <metadata><name>{}</name></metadata>\n",
        escaped_name
    ));
    xml.push_str("  <trk>\n");
    xml.push_str(&format!("    <name>{}</name>\n", escaped_name));
    xml.push_str("    <trkseg>\n");
    for p in points {
        xml.push_str(&format!(
            "      <trkpt lat=\"{:.6}\" lon=\"{:.6}\"></trkpt>\n",
            p.lat, p.lng
        ));
    }
    xml.push_str("    </trkseg>\n");
    xml.push_str("  </trk>\n");
    xml.push_str("</gpx>\n");
    xml
}
