//! Minimal GPX 1.1 import/export (trkpt, rtept and wpt elements).
use crate::LatLng;

fn extract_attr(tag_str: &str, attr_name: &str) -> Option<f64> {
    let mut search_from = 0;
    while let Some(idx) = tag_str[search_from..].find(attr_name) {
        let abs_idx = search_from + idx;
        let before_ok = abs_idx == 0 || tag_str.as_bytes()[abs_idx - 1].is_ascii_whitespace();
        let after_idx = abs_idx + attr_name.len();
        let rem = &tag_str[after_idx..];
        let trimmed = rem.trim_start();
        if before_ok && trimmed.starts_with('=') {
            let after_eq = trimmed[1..].trim_start();
            if let Some(quote) = after_eq.chars().next() {
                if quote == '"' || quote == '\'' {
                    let val_str = &after_eq[quote.len_utf8()..];
                    if let Some(end_quote) = val_str.find(quote) {
                        let val = &val_str[..end_quote];
                        if let Ok(num) = val.trim().parse::<f64>() {
                            return Some(num);
                        }
                    }
                }
            }
        }
        search_from = abs_idx + attr_name.len();
    }
    None
}

fn extract_points_for_tag(xml: &str, tag: &str) -> Vec<LatLng> {
    let open_pat = format!("<{}", tag);
    let mut points = Vec::new();
    let mut search_from = 0;
    while let Some(idx) = xml[search_from..].find(&open_pat) {
        let tag_start = search_from + idx;
        let after_name = tag_start + open_pat.len();
        if after_name < xml.len() {
            let next_ch = xml.as_bytes()[after_name];
            if next_ch.is_ascii_whitespace() || next_ch == b'>' || next_ch == b'/' {
                if let Some(close_idx) = xml[after_name..].find('>') {
                    let tag_body = &xml[after_name..after_name + close_idx];
                    let lat_opt = extract_attr(tag_body, "lat");
                    let lon_opt =
                        extract_attr(tag_body, "lon").or_else(|| extract_attr(tag_body, "lng"));
                    if let (Some(lat), Some(lng)) = (lat_opt, lon_opt) {
                        let pt = LatLng::new(lat, lng);
                        if pt.is_valid() {
                            points.push(pt);
                        }
                    }
                }
            }
        }
        search_from = after_name;
    }
    points
}

fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Parse all `<trkpt>`, then `<rtept>`, then `<wpt>` (first non-empty kind wins) lat/lon attributes.
pub fn parse_gpx(xml: &str) -> anyhow::Result<Vec<LatLng>> {
    let trkpts = extract_points_for_tag(xml, "trkpt");
    if !trkpts.is_empty() {
        return Ok(trkpts);
    }
    let rtepts = extract_points_for_tag(xml, "rtept");
    if !rtepts.is_empty() {
        return Ok(rtepts);
    }
    let wpts = extract_points_for_tag(xml, "wpt");
    if !wpts.is_empty() {
        return Ok(wpts);
    }
    anyhow::bail!("no valid track, route, or waypoint coordinates found in GPX")
}

/// Serialise as a single-track GPX 1.1 document.
pub fn to_gpx(name: &str, path: &[LatLng]) -> String {
    let escaped_name = escape_xml(name);
    let mut xml = String::with_capacity(256 + path.len() * 64);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<gpx version=\"1.1\" creator=\"BloomAnywhere\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n");
    xml.push_str("  <trk>\n");
    xml.push_str(&format!("    <name>{}</name>\n", escaped_name));
    xml.push_str("    <trkseg>\n");
    for pt in path {
        xml.push_str(&format!(
            "      <trkpt lat=\"{:.7}\" lon=\"{:.7}\"/>\n",
            pt.lat, pt.lng
        ));
    }
    xml.push_str("    </trkseg>\n");
    xml.push_str("  </trk>\n");
    xml.push_str("</gpx>\n");
    xml
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_gpx_and_roundtrip() {
        let pts = vec![
            LatLng::new(25.033964, 121.564468),
            LatLng::new(25.047675, 121.517055),
        ];
        let xml = to_gpx("Test <Route> & 'Trip'", &pts);
        assert!(xml.contains("&lt;Route&gt; &amp; &apos;Trip&apos;"));
        assert!(xml.contains("<trkpt lat=\"25.0339640\" lon=\"121.5644680\"/>"));

        let parsed = parse_gpx(&xml).expect("parse gpx");
        assert_eq!(parsed.len(), 2);
        assert!((parsed[0].lat - pts[0].lat).abs() < 1e-6);
        assert!((parsed[0].lng - pts[0].lng).abs() < 1e-6);
        assert!((parsed[1].lat - pts[1].lat).abs() < 1e-6);
        assert!((parsed[1].lng - pts[1].lng).abs() < 1e-6);
    }

    #[test]
    fn test_parse_attribute_variations() {
        // Lon before lat, single quotes, whitespace
        let xml = r#"
            <gpx>
              <trk>
                <trkseg>
                  <trkpt lon = '121.564468' lat = "25.033964">
                    <ele>10.0</ele>
                  </trkpt>
                  <trkpt lat="25.047675" lon="121.517055"/>
                </trkseg>
              </trk>
            </gpx>
        "#;
        let parsed = parse_gpx(xml).expect("parse trkpt");
        assert_eq!(parsed.len(), 2);
        assert!((parsed[0].lat - 25.033964).abs() < 1e-6);
        assert!((parsed[0].lng - 121.564468).abs() < 1e-6);
    }

    #[test]
    fn test_parse_fallback_hierarchy() {
        // Only rtept
        let xml_rte = r#"
            <gpx>
              <rte>
                <rtept lat="10.0" lon="20.0" />
                <rtept lat="11.0" lon="21.0" />
              </rte>
            </gpx>
        "#;
        let parsed_rte = parse_gpx(xml_rte).expect("parse rte");
        assert_eq!(parsed_rte.len(), 2);
        assert_eq!(parsed_rte[0], LatLng::new(10.0, 20.0));

        // Only wpt
        let xml_wpt = r#"
            <gpx>
              <wpt lat="30.0" lon="40.0"><name>W1</name></wpt>
            </gpx>
        "#;
        let parsed_wpt = parse_gpx(xml_wpt).expect("parse wpt");
        assert_eq!(parsed_wpt.len(), 1);
        assert_eq!(parsed_wpt[0], LatLng::new(30.0, 40.0));

        // trkpt wins over wpt
        let xml_both = r#"
            <gpx>
              <wpt lat="30.0" lon="40.0" />
              <trk><trkseg><trkpt lat="50.0" lon="60.0" /></trkseg></trk>
            </gpx>
        "#;
        let parsed_both = parse_gpx(xml_both).expect("parse trkpt priority");
        assert_eq!(parsed_both.len(), 1);
        assert_eq!(parsed_both[0], LatLng::new(50.0, 60.0));
    }

    #[test]
    fn test_parse_empty_errors() {
        assert!(parse_gpx("<gpx></gpx>").is_err());
        assert!(parse_gpx("not even xml").is_err());
    }
}
