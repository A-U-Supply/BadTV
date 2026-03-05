use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct GdeltResponse {
    pub clips: Option<Vec<Clip>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Clip {
    pub preview_url: String,
    pub ia_show_id: String,
    pub date: String,
    #[allow(dead_code)] // deserialized from API, used for display
    pub station: String,
    pub show: String,
    pub snippet: String,
}

impl Clip {
    /// Parse start/end seconds from preview_url.
    /// URL format: https://archive.org/details/SHOW#start/SECONDS/end/SECONDS
    pub fn time_range(&self) -> Option<(f64, f64)> {
        let fragment = self.preview_url.split('#').nth(1)?;
        let parts: Vec<&str> = fragment.split('/').collect();
        if parts.len() >= 4 && parts[0] == "start" && parts[2] == "end" {
            let start = parts[1].parse::<f64>().ok()?;
            let end = parts[3].parse::<f64>().ok()?;
            Some((start, end))
        } else {
            None
        }
    }

    /// The MP3 download URL for this clip's show.
    pub fn mp3_url(&self) -> String {
        format!(
            "https://archive.org/download/{}/{}.mp3",
            self.ia_show_id, self.ia_show_id
        )
    }
}

/// Search the GDELT TV API for clips containing `word`.
pub async fn search_word(
    client: &reqwest::Client,
    word: &str,
    stations: &[String],
    exclude: &[String],
    base_url: &str,
) -> Result<Vec<Clip>> {
    let default_stations = vec![
        "CNN", "MSNBC", "FOXNEWS", "CNBC", "CSPAN", "BBCNEWS",
        "BLOOMBERG", "FBC",
    ];

    let station_list: Vec<&str> = if stations.is_empty() {
        default_stations
    } else {
        stations.iter().map(|s| s.as_str()).collect()
    };

    let mut all_clips = Vec::new();

    for station in &station_list {
        if exclude.iter().any(|e| e.eq_ignore_ascii_case(station)) {
            continue;
        }

        let query = format!("{} station:{}", word, station);
        let url = format!(
            "{}?query={}&mode=clipgallery&format=json&MAXRECORDS=5",
            base_url,
            urlencoding::encode(&query)
        );

        let resp = client
            .get(&url)
            .send()
            .await
            .context("GDELT API request failed")?;

        let text = resp.text().await?;

        if let Ok(parsed) = serde_json::from_str::<GdeltResponse>(&text) {
            if let Some(clips) = parsed.clips {
                all_clips.extend(clips);
            }
        }
    }

    Ok(all_clips)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clip_time_range() {
        let clip = Clip {
            preview_url: "https://archive.org/details/SHOW#start/3296/end/3331".to_string(),
            ia_show_id: "SHOW".to_string(),
            date: "2017-07-05T18:55:11Z".to_string(),
            station: "CNN".to_string(),
            show: "CNN Newsroom".to_string(),
            snippet: "hello world".to_string(),
        };
        let (start, end) = clip.time_range().unwrap();
        assert!((start - 3296.0).abs() < 0.01);
        assert!((end - 3331.0).abs() < 0.01);
    }

    #[test]
    fn test_clip_mp3_url() {
        let clip = Clip {
            preview_url: String::new(),
            ia_show_id: "CNNW_20170705_180000_Show".to_string(),
            date: String::new(),
            station: String::new(),
            show: String::new(),
            snippet: String::new(),
        };
        assert_eq!(
            clip.mp3_url(),
            "https://archive.org/download/CNNW_20170705_180000_Show/CNNW_20170705_180000_Show.mp3"
        );
    }

    #[test]
    fn test_clip_time_range_no_fragment() {
        let clip = Clip {
            preview_url: "https://archive.org/details/SHOW".to_string(),
            ia_show_id: "SHOW".to_string(),
            date: String::new(),
            station: String::new(),
            show: String::new(),
            snippet: String::new(),
        };
        assert!(clip.time_range().is_none());
    }
}
