use log::{info, error};
use env_logger::{Builder, Env};
use clap::Parser;
use serde_json::Value;

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// The name of the package to install
    #[arg(value_parser = sanitize_name)]
    name: String
}

fn sanitize_name(s: &str) -> Result<String, String> {
    let mut result = s.to_string();

    if !s.contains('/') {
        result = format!("{s}/{s}");
    }

    Ok(result)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Builder::from_env(Env::default().default_filter_or("info")).init();

    let args =  Args::parse();

    info!("Installing {}", args.name);

    let url = format!("https://api.github.com/repos/{}/releases/latest", args.name);
    let client = reqwest::Client::new();

    info!("Checking GitHub's latest releases at {}", url);

    let response = client
            .get(&url)
            .header("User-Agent", "bini")
            .send()
            .await?
            .json::<Value>()
            .await?;

    let Some(tag) = response["tag_name"].as_str() else {
        error!("No release found");
        return Err("No release found".into());
    };

    info!("Found release with tag {}", tag);

    info!("Checking release for linux x86 assets");

    let mut latest_name = None;
    let mut latest_url = None;
    let mut latest_date = None;

    if let Some(assets) = response["assets"].as_array() {
        for asset in assets {
            let name = match asset["name"].as_str() {
                Some(n) => n,
                None => continue,
            };

            info!("Checking asset: {}", name);

            let name_lower = name.to_lowercase();

            // Test 1: Must contain 'linux'
            if !name_lower.contains("linux") {
                continue;
            }

            // Test 2: Must contain 'amd64', 'x86_64', or 'x64'
            if !name_lower.contains("amd64")
                && !name_lower.contains("x86_64")
                && !name_lower.contains("x64")
            {
                continue;
            }

            // Test 3: If contains '.', must end with .gz, .tar.gz, .tgz, or .zip
            if name.contains('.')
                && !name.ends_with(".gz")
                && !name.ends_with(".tar.gz")
                && !name.ends_with(".tgz")
                && !name.ends_with(".zip")
            {
                continue;
            }

            latest_name = asset["name"].as_str().map(String::from);
            latest_url = asset["browser_download_url"].as_str().map(String::from);
            latest_date = asset["updated_at"].as_str().map(String::from);
            break;
        }
    }

    let (Some(name), Some(url), Some(date)) = (latest_name, latest_url, latest_date) else {
        error!("No linux x86 asset found in release");
        return Err("No compatible linux x86 asset found in release".into());
    };

    info!("Found compatible asset: {} ({})", name, date);

    info!("Latest URL found: {}", url);

    Ok(())
}
