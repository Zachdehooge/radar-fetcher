use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use futures::future::join_all;
use reqwest::Client;
use scraper::{Html, Selector};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use url::Url;

// Add these dependencies to your Cargo.toml:
// [dependencies]
// reqwest = { version = "0.12", features = ["stream"] }
// tokio = { version = "1.0", features = ["full"] }
// scraper = "0.20"
// url = "2.5"
// futures = "0.3"

#[derive(Debug)]
struct DownloadProgress {
    total: usize,
    current: AtomicUsize,
    last_file: Arc<Mutex<String>>,
}

impl DownloadProgress {
    fn new(total: usize) -> Self {
        Self {
            total,
            current: AtomicUsize::new(0),
            last_file: Arc::new(Mutex::new(String::new())),
        }
    }

    fn increment(&self, filename: &str) {
        let current = self.current.fetch_add(1, Ordering::SeqCst) + 1;
        {
            let mut last_file = self.last_file.lock().unwrap();
            *last_file = filename.to_string();
        }
        self.print_progress(current, filename);
    }

    fn print_progress(&self, current: usize, filename: &str) {
        let percentage = (current as f64 / self.total as f64) * 100.0;
        print!("\rDownloading Files: {}/{} ({:.1}%) | Last: {} ",
               current, self.total, percentage, filename);
        io::stdout().flush().unwrap();

        if current == self.total {
            println!("\nDownload Complete!");
        }
    }
}

async fn fetch_download_links(radar_url: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let client = Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (HTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
        .build()?;
    let response = client.get(radar_url).send().await?;
    let html = response.text().await?;

    println!("Fetching from URL: {}", radar_url);

    let document = Html::parse_document(&html);

    // Try multiple selectors that might contain download links
    let selectors = vec![
        "div.bdpLink a",
        "a[href*='.gz']",
        "a[href*='.tar']",
        "a[href*='.bz2']",
        "a[href*='download']",
        "a[href*='V06']", // NEXRAD files often have V06 in them
        "a[href*='AAL2']", // Product code from URL
        "table a", // Links might be in a table
        ".download a",
        ".file-link a",
        "a", // Fallback to all links
    ];

    let mut links = Vec::new();

    for selector_str in &selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            let mut found_links = 0;
            for element in document.select(&selector) {
                if let Some(href) = element.value().attr("href") {
                    // Filter for likely data file links
                    if href.contains(".gz") ||
                        href.contains(".tar") ||
                        href.contains(".bz2") ||
                        href.contains("V06") ||
                        href.contains("AAL2") ||
                        (href.starts_with("http") && href.contains("download")) {
                        let absolute_url = resolve_url(radar_url, href)?;
                        if !links.contains(&absolute_url) {
                            links.push(absolute_url);
                            found_links += 1;
                        }
                    }
                }
            }
            if found_links > 0 {
                println!("Found {} links using selector: {}", found_links, selector_str);
                break; // Use the first selector that finds links
            }
        }
    }

    // Debug: print the first few characters of HTML to see structure
    if links.is_empty() {
        println!("No download links found. HTML preview (first 1000 chars):");
        println!("{}", &html[..std::cmp::min(1000, html.len())]);

        // Also try to find any links at all for debugging
        if let Ok(all_links_selector) = Selector::parse("a") {
            println!("\nAll links found on page:");
            for (i, element) in document.select(&all_links_selector).enumerate() {
                if i > 10 { // Limit output
                    println!("... (showing first 10 links only)");
                    break;
                }
                if let Some(href) = element.value().attr("href") {
                    let text = element.text().collect::<Vec<_>>().join(" ");
                    println!("  {} -> {}", text.trim(), href);
                }
            }
        }
    }

    println!("Total download links found: {}", links.len());
    Ok(links)
}

fn resolve_url(base_url: &str, link: &str) -> Result<String, Box<dyn std::error::Error>> {
    let base = Url::parse(base_url)?;
    let resolved = base.join(link)?;
    Ok(resolved.to_string())
}

async fn download_file(
    url: &str,
    output_dir: &str,
    progress: Arc<DownloadProgress>,
    client: &Client,
) -> Result<String, Box<dyn std::error::Error>> {
    let response = client.get(url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .send()
        .await?;

    let filename = Path::new(url)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown_file");

    let file_path = Path::new(output_dir).join(filename);
    let mut file = File::create(&file_path).await?;

    let bytes = response.bytes().await?;
    file.write_all(&bytes).await?;

    progress.increment(filename);
    Ok(filename.to_string())
}

async fn download_files(links: Vec<String>, output_dir: &str) -> Vec<String> {
    let progress = Arc::new(DownloadProgress::new(links.len()));
    let client = Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(std::time::Duration::from_secs(300)) // 5-minute timeout
        .build()
        .expect("Failed to create HTTP client");
    let max_concurrent = 50;

    // Create semaphore using tokio::sync::Semaphore
    let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent));

    let mut tasks = Vec::new();

    for link in links {
        let progress_clone = Arc::clone(&progress);
        let client_clone = client.clone();
        let output_dir_clone = output_dir.to_string();
        let semaphore_clone = Arc::clone(&semaphore);

        let task = tokio::spawn(async move {
            let _permit = semaphore_clone.acquire().await.unwrap();
            match download_file(&link, &output_dir_clone, progress_clone, &client_clone).await {
                Ok(filename) => Some(filename),
                Err(e) => {
                    eprintln!("\nError downloading {}: {}", link, e);
                    None
                }
            }
        });

        tasks.push(task);
    }

    let results = join_all(tasks).await;
    let downloaded_files: Vec<String> = results
        .into_iter()
        .filter_map(|result| result.ok().flatten())
        .collect();

    println!(); // New line after progress
    downloaded_files
}

fn prompt_input(prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
    print!("{}", prompt);
    io::stdout().flush()?;

    let stdin = io::stdin();
    let mut line = String::new();
    stdin.read_line(&mut line)?;

    Ok(line.trim().to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let radar = prompt_input("Enter radar site (KHTX): ")?;
    let radar = radar.to_uppercase();
    let month = prompt_input("Enter month (03): ")?;
    let day = prompt_input("Enter day (15): ")?;
    let year = prompt_input("Enter year (2025): ")?;

    let url = format!(
        "https://www.ncdc.noaa.gov/nexradinv/bdp-download.jsp?id={}&yyyy={}&mm={}&dd={}&product=AAL2",
        radar, year, month, day
    );

    let output_dir = format!("{}_{}_{}_{}", radar, year, month, day);

    fs::create_dir_all(&output_dir)?;

    println!("Fetching download links...");
    let links = fetch_download_links(&url).await?;

    if links.is_empty() {
        println!("No download links found. Please check your input parameters.");
        return Ok(());
    }

    println!("Found {} files to download", links.len());
    let downloaded_files = download_files(links, &output_dir).await;

    println!("Total files downloaded: {}", downloaded_files.len());
    println!("Files saved in: {}", output_dir);

    Ok(())
}