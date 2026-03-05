use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

const MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin";

/// Resolve a model path, expanding `~/` to the user's home directory.
pub fn resolve_model_path(path: &str) -> Result<PathBuf> {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = dirs::home_dir().context("Cannot determine home directory")?;
        Ok(home.join(rest))
    } else {
        Ok(PathBuf::from(path))
    }
}

/// Download the Whisper model to the given destination.
pub async fn download_model(client: &reqwest::Client, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).context("Failed to create model directory")?;
    }
    eprintln!("Downloading Whisper model to {}...", dest.display());
    eprintln!("This is a one-time download (~142MB).");
    let resp = client
        .get(MODEL_URL)
        .send()
        .await
        .context("Failed to download whisper model")?;
    if !resp.status().is_success() {
        bail!("Model download failed with status: {}", resp.status());
    }
    let bytes = resp.bytes().await?;
    std::fs::write(dest, &bytes).context("Failed to write model file")?;
    eprintln!("Model downloaded successfully.");
    Ok(())
}

/// Check that the model file exists, returning a helpful error if not.
pub fn ensure_model_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!(
            "Whisper model not found at: {}\nRun `badtv --download-model` to download it.",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_absolute_path() {
        let path = resolve_model_path("/tmp/model.bin").unwrap();
        assert_eq!(path, PathBuf::from("/tmp/model.bin"));
    }

    #[test]
    fn test_resolve_tilde_path() {
        let path = resolve_model_path("~/.badtv/model.bin").unwrap();
        assert!(path.to_str().unwrap().contains(".badtv/model.bin"));
        assert!(!path.to_str().unwrap().starts_with("~"));
    }

    #[test]
    fn test_ensure_model_missing() {
        let result = ensure_model_exists(Path::new("/nonexistent/model.bin"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("badtv --download-model"));
    }
}
