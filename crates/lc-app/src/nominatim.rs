use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub name: String,
    pub lat: f64,
    pub lng: f64,
}

#[derive(Deserialize)]
struct NominatimItem {
    display_name: String,
    lat: String,
    lon: String,
}

pub async fn search_location(
    client: &reqwest::Client,
    query: &str,
) -> anyhow::Result<Vec<SearchResult>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }

    let url = "https://nominatim.openstreetmap.org/search";
    let res = client
        .get(url)
        .query(&[("format", "json"), ("q", query), ("limit", "10")])
        .header("User-Agent", "BloomAnywhere/0.1.0")
        .timeout(Duration::from_secs(5))
        .send()
        .await?;

    if !res.status().is_success() {
        anyhow::bail!("Nominatim search failed with HTTP {}", res.status());
    }

    let items: Vec<NominatimItem> = res.json().await?;
    let results = items
        .into_iter()
        .filter_map(|item| {
            let lat = item.lat.parse::<f64>().ok()?;
            let lng = item.lon.parse::<f64>().ok()?;
            Some(SearchResult {
                name: item.display_name,
                lat,
                lng,
            })
        })
        .collect();

    Ok(results)
}
