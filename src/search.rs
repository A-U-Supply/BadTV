use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Clip {
    pub identifier: String,
    pub show: String,
    pub station: String,
    pub start_secs: f64,
    pub end_secs: f64,
}

impl Clip {
    pub fn mp3_url(&self) -> String {
        format!(
            "https://archive.org/download/{}/{}.mp3",
            self.identifier, self.identifier
        )
    }

    pub fn srt_urls(&self) -> Vec<String> {
        vec![
            format!(
                "https://archive.org/download/{}/{}.cc5.srt",
                self.identifier, self.identifier
            ),
            format!(
                "https://archive.org/download/{}/{}.cc1.srt",
                self.identifier, self.identifier
            ),
        ]
    }
}

/// A precise caption line from an SRT file.
#[derive(Debug, Clone)]
pub struct CaptionHit {
    pub start_secs: f64,
    pub end_secs: f64,
    pub text: String,
}

/// Download and parse an SRT file, returning caption lines that contain the target word.
pub async fn find_word_in_srt(
    client: &reqwest::Client,
    clip: &Clip,
    word: &str,
) -> Result<Vec<CaptionHit>> {
    let mut text = None;
    for srt_url in clip.srt_urls() {
        let resp = match tokio::time::timeout(
            std::time::Duration::from_secs(3),
            client.get(&srt_url).send(),
        )
        .await
        {
            Ok(Ok(r)) if r.status().is_success() => r,
            _ => continue,
        };
        if let Ok(body) = resp.text().await {
            text = Some(body);
            break;
        }
    }
    let text = text.context("No SRT caption file found")?;
    let target = word.to_lowercase();

    let mut hits = Vec::new();
    let mut lines = text.lines().peekable();

    while lines.peek().is_some() {
        // Skip sequence number.
        let seq = match lines.next() {
            Some(s) => s.trim().to_string(),
            None => break,
        };
        if seq.is_empty() {
            continue;
        }
        // Sequence number should be numeric.
        if seq.parse::<u64>().is_err() {
            continue;
        }

        // Timestamp line: "HH:MM:SS,mmm --> HH:MM:SS,mmm"
        let timestamp_line = match lines.next() {
            Some(s) => s.trim().to_string(),
            None => break,
        };

        let (start, end) = match parse_srt_timestamps(&timestamp_line) {
            Some(t) => t,
            None => continue,
        };

        // Collect subtitle text lines until blank line.
        let mut caption_text = String::new();
        for line in lines.by_ref() {
            let line = line.trim();
            if line.is_empty() {
                break;
            }
            if !caption_text.is_empty() {
                caption_text.push(' ');
            }
            caption_text.push_str(line);
        }

        // Check if the caption contains our word.
        let caption_lower = caption_text.to_lowercase();
        let has_word = caption_lower.split_whitespace().any(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric()) == target
        });

        if has_word {
            hits.push(CaptionHit {
                start_secs: start,
                end_secs: end,
                text: caption_text,
            });
        }
    }

    // Sort by word count — shorter captions are easier to extract from.
    hits.sort_by_key(|h| h.text.split_whitespace().count());

    Ok(hits)
}

/// Parse SRT timestamp line like "00:24:51,000 --> 00:24:56,000" into seconds.
fn parse_srt_timestamps(line: &str) -> Option<(f64, f64)> {
    let parts: Vec<&str> = line.split("-->").collect();
    if parts.len() != 2 {
        return None;
    }
    let start = parse_srt_time(parts[0].trim())?;
    let end = parse_srt_time(parts[1].trim())?;
    Some((start, end))
}

fn parse_srt_time(s: &str) -> Option<f64> {
    // Format: HH:MM:SS,mmm or HH:MM:SS.mmm
    let s = s.replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let hours: f64 = parts[0].parse().ok()?;
    let minutes: f64 = parts[1].parse().ok()?;
    let seconds: f64 = parts[2].parse().ok()?;
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

#[derive(Debug, Deserialize)]
struct IaResponse {
    response: Option<IaResponseBody>,
}

#[derive(Debug, Deserialize)]
struct IaResponseBody {
    body: Option<IaBody>,
}

#[derive(Debug, Deserialize)]
struct IaBody {
    hits: Option<IaHits>,
}

#[derive(Debug, Deserialize)]
struct IaHits {
    hits: Option<Vec<IaHit>>,
}

#[derive(Debug, Deserialize)]
struct IaHit {
    fields: Option<IaFields>,
}

#[derive(Debug, Deserialize)]
struct IaFields {
    identifier: Option<String>,
    title: Option<String>,
    creator: Option<Vec<String>>,
    #[serde(rename = "__href__")]
    href: Option<String>,
}

/// Search the Internet Archive TV News caption index.
pub async fn search_word(
    client: &reqwest::Client,
    word: &str,
    stations: &[String],
    exclude: &[String],
    base_url: &str,
) -> Result<Vec<Clip>> {
    let url = format!(
        "{}?service_backend=tvs&user_query={}&hits_per_page=50",
        base_url,
        urlencoding::encode(word)
    );

    let resp = client
        .get(&url)
        .send()
        .await
        .context("IA TV search request failed")?;
    let text = resp.text().await?;

    let parsed: IaResponse = serde_json::from_str(&text).unwrap_or(IaResponse { response: None });

    let hits = parsed
        .response
        .and_then(|r| r.body)
        .and_then(|b| b.hits)
        .and_then(|h| h.hits)
        .unwrap_or_default();

    let mut clips: Vec<Clip> = hits
        .into_iter()
        .filter_map(|hit| {
            let fields = hit.fields?;
            let identifier = fields.identifier?;
            let title = fields.title.unwrap_or_default();
            let station = fields
                .creator
                .and_then(|c| c.into_iter().next())
                .unwrap_or_default();

            // Parse start/end from href: /details/ID/start/321/end/381?q=...
            let href = fields.href?;
            let (start, end) = parse_href_times(&href)?;

            Some(Clip {
                identifier,
                show: title,
                station,
                start_secs: start,
                end_secs: end,
            })
        })
        .collect();

    if !stations.is_empty() {
        clips.retain(|c| {
            stations
                .iter()
                .any(|s| s.eq_ignore_ascii_case(&c.station))
        });
    }
    if !exclude.is_empty() {
        clips.retain(|c| {
            !exclude
                .iter()
                .any(|e| e.eq_ignore_ascii_case(&c.station))
        });
    }

    Ok(clips)
}

/// Check which clips have both MP3 and SRT files available.
/// Probes all clips in parallel using HEAD requests.
pub async fn filter_available_clips(client: &reqwest::Client, clips: &[Clip]) -> Vec<Clip> {
    use futures::future::join_all;

    let checks: Vec<_> = clips
        .iter()
        .map(|clip| {
            let client = client.clone();
            let mp3_url = clip.mp3_url();
            let srt_urls = clip.srt_urls();
            let clip = clip.clone();
            async move {
                // Check MP3 exists.
                let mp3_ok = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    client.head(&mp3_url).send(),
                )
                .await
                .ok()
                .and_then(|r| r.ok())
                .map(|r| r.status().is_success())
                .unwrap_or(false);

                if !mp3_ok {
                    return None;
                }

                // Check SRT exists (try cc5 then cc1).
                for srt_url in &srt_urls {
                    let srt_ok = tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        client.head(srt_url).send(),
                    )
                    .await
                    .ok()
                    .and_then(|r| r.ok())
                    .map(|r| r.status().is_success())
                    .unwrap_or(false);

                    if srt_ok {
                        return Some(clip);
                    }
                }
                None
            }
        })
        .collect();

    let results = join_all(checks).await;
    results.into_iter().flatten().collect()
}

/// Parse start/end seconds from IA href like `/details/ID/start/321/end/381?q=...`
fn parse_href_times(href: &str) -> Option<(f64, f64)> {
    let path = href.split('?').next()?;
    let parts: Vec<&str> = path.split('/').collect();
    // parts: ["", "details", "ID", "start", "321", "end", "381"]
    let start_idx = parts.iter().position(|&p| p == "start")?;
    let end_idx = parts.iter().position(|&p| p == "end")?;
    let start = parts.get(start_idx + 1)?.parse::<f64>().ok()?;
    let end = parts.get(end_idx + 1)?.parse::<f64>().ok()?;
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_href_times() {
        let href = "/details/CNBC_20090829_080000_Mad_Money/start/321/end/381?q=buy";
        let (start, end) = parse_href_times(href).unwrap();
        assert!((start - 321.0).abs() < 0.01);
        assert!((end - 381.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_href_times_no_times() {
        assert!(parse_href_times("/details/SHOW").is_none());
    }

    #[test]
    fn test_clip_mp3_url() {
        let clip = Clip {
            identifier: "CNNW_20170705_180000_Show".to_string(),
            show: String::new(),
            station: String::new(),
            start_secs: 0.0,
            end_secs: 60.0,
        };
        assert_eq!(
            clip.mp3_url(),
            "https://archive.org/download/CNNW_20170705_180000_Show/CNNW_20170705_180000_Show.mp3"
        );
    }

}
