use anyhow::Result;
use ocr_rs::OcrEngine;
use once_cell::sync::OnceCell;
use std::path::Path;
use std::sync::Mutex;
use tokio::task;

pub struct OcrResult {
    pub text: String,
}

// Model file names
const DET_MODEL: &str = "PP-OCRv5_mobile_det.mnn";
const REC_MODEL: &str = "PP-OCRv5_mobile_rec.mnn";
const CHARSET_FILE: &str = "ppocr_keys_v5.txt";

// GitHub raw URLs for downloading models
const MODEL_BASE_URL: &str = "https://raw.githubusercontent.com/zibo-chen/rust-paddle-ocr/next/models";

// Static OCR engine instance
static OCR_ENGINE: OnceCell<OcrEngine> = OnceCell::new();

// Mutex to ensure only one thread initializes the engine at a time
static INIT_LOCK: Mutex<()> = Mutex::new(());

/// Suppress stdout and stderr during closure execution
fn suppress_output<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    use std::fs::OpenOptions;
    use std::os::fd::AsRawFd;
    
    // Save original stdout and stderr
    let stdout_fd = unsafe { libc::dup(1) };
    let stderr_fd = unsafe { libc::dup(2) };
    
    // Open /dev/null
    let dev_null = OpenOptions::new()
        .write(true)
        .open("/dev/null")
        .expect("Failed to open /dev/null");
    
    let dev_null_fd = dev_null.as_raw_fd();
    
    // Redirect stdout and stderr to /dev/null
    unsafe {
        libc::dup2(dev_null_fd, 1);
        libc::dup2(dev_null_fd, 2);
    }
    
    // Execute the closure
    let result = f();
    
    // Restore stdout and stderr
    unsafe {
        libc::dup2(stdout_fd, 1);
        libc::dup2(stderr_fd, 2);
        libc::close(stdout_fd);
        libc::close(stderr_fd);
    }
    
    result
}

/// Initialize the OCR engine lazily
async fn get_ocr_engine() -> Result<&'static OcrEngine> {
    // Use a lock to ensure only one thread initializes at a time
    let _guard = INIT_LOCK.lock().unwrap();
    
    if let Some(engine) = OCR_ENGINE.get() {
        return Ok(engine);
    }
    
    // Ensure models are downloaded
    let models_dir = Path::new("./models");
    
    if !models_dir.exists() {
        tokio::fs::create_dir_all(models_dir).await?;
    }
    
    let det_path = models_dir.join(DET_MODEL);
    let rec_path = models_dir.join(REC_MODEL);
    let charset_path = models_dir.join(CHARSET_FILE);
    
    // Download models if they don't exist
    if !det_path.exists() {
        download_model(DET_MODEL, &det_path).await?;
    }
    
    if !rec_path.exists() {
        download_model(REC_MODEL, &rec_path).await?;
    }
    
    if !charset_path.exists() {
        download_model(CHARSET_FILE, &charset_path).await?;
    }
    
    // Create OCR engine in a blocking task since MNN operations block
    let det_path_str = det_path.to_str().unwrap().to_string();
    let rec_path_str = rec_path.to_str().unwrap().to_string();
    let charset_path_str = charset_path.to_str().unwrap().to_string();
    
    let engine = task::spawn_blocking(move || {
        suppress_output(|| {
            OcrEngine::new(
                &det_path_str,
                &rec_path_str,
                &charset_path_str,
                None,
            ).map_err(|e| anyhow::anyhow!("Failed to create OCR engine: {:?}", e))
        })
    }).await.map_err(|e| anyhow::anyhow!("Task join error: {}", e))??;
    
    OCR_ENGINE.set(engine).map_err(|_| anyhow::anyhow!("Failed to set OCR engine"))?;
    
    Ok(OCR_ENGINE.get().unwrap())
}

/// Download a model file from GitHub
async fn download_model(filename: &str, dest_path: &Path) -> Result<()> {
    let url = format!("{}/{}", MODEL_BASE_URL, filename);
    
    let response = reqwest::get(&url).await
        .map_err(|e| anyhow::anyhow!("Failed to download {}: {}", filename, e))?;
    
    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to download {}: HTTP {}",
            filename,
            response.status()
        ));
    }
    
    let bytes = response.bytes().await
        .map_err(|e| anyhow::anyhow!("Failed to read response for {}: {}", filename, e))?;
    
    tokio::fs::write(dest_path, &bytes).await
        .map_err(|e| anyhow::anyhow!("Failed to write file {}: {}", dest_path.display(), e))?;
    
    Ok(())
}

pub async fn recognize_image_from_path(path: &str) -> Result<OcrResult> {
    let path = Path::new(path);
    if !path.exists() {
        return Err(anyhow::anyhow!("Image file not found: {}", path.display()));
    }
    
    // Load image
    let image = image::open(path)
        .map_err(|e| anyhow::anyhow!("Failed to load image: {}", e))?;
    
    // Get OCR engine
    let engine = get_ocr_engine().await?;
    
    // Perform OCR in a blocking task
    let results = task::spawn_blocking(move || {
        engine.recognize(&image)
            .map_err(|e| anyhow::anyhow!("OCR recognition failed: {:?}", e))
    }).await.map_err(|e| anyhow::anyhow!("Task join error: {}", e))??;
    
    // Combine all text results
    let text = results.iter()
        .map(|r| r.text.clone())
        .collect::<Vec<_>>()
        .join("\n");
    
    Ok(OcrResult { text })
}

pub async fn recognize_image(image_data: &[u8]) -> Result<OcrResult> {
    // Load image from bytes
    let image = image::load_from_memory(image_data)
        .map_err(|e| anyhow::anyhow!("Failed to load image from memory: {}", e))?;
    
    // Get OCR engine
    let engine = get_ocr_engine().await?;
    
    // Perform OCR in a blocking task with suppressed output
    let results = task::spawn_blocking(move || {
        suppress_output(|| {
            engine.recognize(&image)
                .map_err(|e| anyhow::anyhow!("OCR recognition failed: {:?}", e))
        })
    }).await.map_err(|e| anyhow::anyhow!("Task join error: {}", e))??;
    
    // Combine all text results
    let text = results.iter()
        .map(|r| r.text.clone())
        .collect::<Vec<_>>()
        .join("\n");
    
    Ok(OcrResult { text })
}

pub async fn recognize_image_from_rgba(
    width: u32,
    height: u32,
    rgba_bytes: &[u8],
) -> Result<OcrResult> {
    // Create image from raw RGBA bytes
    let rgba_image = image::RgbaImage::from_raw(width, height, rgba_bytes.to_vec())
        .ok_or_else(|| anyhow::anyhow!("Failed to create image from raw bytes"))?;

    let dynamic_image = image::DynamicImage::ImageRgba8(rgba_image);

    // Get OCR engine
    let engine = get_ocr_engine().await?;

    // Perform OCR in a blocking task
    let results = task::spawn_blocking(move || {
        suppress_output(|| {
            engine
                .recognize(&dynamic_image)
                .map_err(|e| anyhow::anyhow!("OCR recognition failed: {:?}", e))
        })
    })
    .await
    .map_err(|e| anyhow::anyhow!("Task join error: {}", e))??;

    // Combine all text results
    let text = results
        .iter()
        .map(|r| r.text.clone())
        .collect::<Vec<_>>()
        .join("\n");

    Ok(OcrResult { text })
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_is_image_path() {
        assert!(Path::new("test.png").extension().is_some());
        assert!(Path::new("test.jpg").extension().is_some());
    }
}
