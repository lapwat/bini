use clap::{Parser, Subcommand};
use dirs::{data_dir, state_dir};
use env_logger::{Builder, Env};
use log::{error, info, warn};
use serde_json::Value;
use std::env;
use std::env::consts::{ARCH, OS};
use std::fs::{self, File};
use std::io::copy;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use walkdir::WalkDir;

#[derive(Parser)]
#[command(
    version,
    about,
    after_help = "Examples:\n  bini install sharkdp/bat\n  bini install burntsushi/ripgrep --as rg\n  bini i sharkdp/bat\n  bini sharkdp/bat"
)]
struct Args {
    /// The name of the package to install
    #[arg(value_parser = sanitize_name)]
    name: Option<String>,

    /// Install the binary under a different name
    #[arg(long = "as", requires = "name")]
    as_name: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Install from GitHub repository
    #[command(
        visible_aliases = ["i"],
        after_help = "Examples:\n  bini install sharkdp/bat\n  bini install burntsushi/ripgrep --as rg\n  bini i sharkdp/bat\n  bini sharkdp/bat"
    )]
    Install {
        /// The name of the package to install
        #[arg(value_parser = sanitize_name)]
        name: String,

        /// Install the binary under a different name
        #[arg(long = "as")]
        as_name: Option<String>,
    },

    /// List installed binaries
    #[command(visible_aliases = ["l"])]
    List,

    /// Update all installed binaries
    #[command(visible_aliases = ["u"], after_help = "Examples:\n  bini update\n  bini")]
    Update,

    /// Remove installed binary
    #[command(
        visible_aliases = ["r", "rm", "delete", "uninstall"],
        after_help = "Examples:\n  bini remove rg"
    )]
    Remove {
        /// The name of the binary to remove
        name: String,
    },
}

fn sanitize_name(s: &str) -> Result<String, String> {
    let mut result = s.to_string();

    if !s.contains('/') {
        result = format!("{s}/{s}");
    }

    Ok(result)
}

fn is_in_path(dest_folder: &Path) -> bool {
    let path_var = match env::var_os("PATH") {
        Some(var) => var,
        None => return false,
    };

    let target = dest_folder
        .canonicalize()
        .unwrap_or_else(|_| dest_folder.to_path_buf());

    env::split_paths(&path_var).any(|p| p.canonicalize().unwrap_or(p) == target)
}

fn find_executable(tmp_dir: &Path) -> Option<PathBuf> {
    for entry in WalkDir::new(tmp_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        let is_exe = path
            .metadata()
            .map(|m| m.is_file() && (m.permissions().mode() & 0o111 != 0))
            .unwrap_or(false);

        if is_exe {
            return Some(path.to_path_buf());
        }
    }
    None
}

fn make_executable(path: &Path) -> std::io::Result<()> {
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(perms.mode() | 0o111);
    fs::set_permissions(path, perms)
}

/// Returns true if a lowercased asset name matches the given OS, covering the
/// names commonly used for it in release assets.
fn os_matches(name: &str, os: &str) -> bool {
    match os {
        "linux" => name.contains("linux"),
        "macos" => name.contains("darwin") || name.contains("macos") || name.contains("osx"),
        "windows" => name.contains("windows") || name.contains("win"),
        _ => name.contains(os),
    }
}

/// Returns true if a lowercased asset name matches the given architecture,
/// covering the names commonly used for it in release assets. Shorter aliases
/// like "x86" or "arm" are only accepted when the longer variants (which they
/// are substrings of) are absent.
fn arch_matches(name: &str, arch: &str) -> bool {
    match arch {
        "x86_64" => name.contains("x86_64") || name.contains("amd64") || name.contains("x64"),
        "aarch64" => name.contains("aarch64") || name.contains("arm64"),
        "x86" => {
            !name.contains("x86_64")
                && !name.contains("amd64")
                && (name.contains("x86")
                    || name.contains("i386")
                    || name.contains("i686")
                    || name.contains("386"))
        }
        "arm" => {
            !name.contains("aarch64")
                && !name.contains("arm64")
                && (name.contains("armv7") || name.contains("armhf") || name.contains("arm"))
        }
        _ => name.contains(arch),
    }
}

/// Formats an `OffsetDateTime` as YYYY-MM-DD.
fn format_date(datetime: OffsetDateTime) -> String {
    let date = datetime.date();
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

/// Parses an RFC 3339 timestamp (like GitHub's asset `updated_at`) and formats
/// it as YYYY-MM-DD. Falls back to the raw string if it can't be parsed.
fn format_asset_date(date: &str) -> String {
    match OffsetDateTime::parse(date, &Rfc3339) {
        Ok(datetime) => format_date(datetime),
        Err(_) => date.to_string(),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Builder::from_env(Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    let installation_directory = data_dir().ok_or("No data dir")?.join("bini/bin");
    if !installation_directory.exists() {
        std::fs::create_dir_all(&installation_directory)?;
        info!(
            "Created installation directory {}",
            installation_directory.display()
        );
    }

    if !is_in_path(&installation_directory) {
        warn!(
            "Consider adding {} to your PATH",
            installation_directory.display()
        )
    }

    let args = Args::parse();

    let (package, as_name) = match args.command {
        Some(Command::List) => return list_binaries(&installation_directory),
        Some(Command::Install { name, as_name }) => (name, as_name),
        Some(Command::Update) => return update_binaries(&installation_directory),
        Some(Command::Remove { name }) => return remove_binary(&name, &installation_directory),
        None => {
            if let Some(name) = args.name {
                (name, args.as_name)
            } else {
                return update_binaries(&installation_directory);
            }
        }
    };

    install(&package, as_name.as_deref(), &installation_directory)
}

fn install(
    package: &str,
    as_name: Option<&str>,
    installation_directory: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let binary_name = match as_name {
        Some(alias) => alias,
        None => package.split('/').last().unwrap(),
    };
    let binary_path = installation_directory.join(binary_name);

    info!("Installing {} as {}", package, binary_name);

    let url = format!("https://api.github.com/repos/{}/releases/latest", package);
    let client = reqwest::blocking::Client::new();
    info!("Checking GitHub's latest release at {}", url);

    let response = client
        .get(&url)
        .header("User-Agent", "bini")
        .send()?
        .json::<Value>()?;

    let Some(tag) = response["tag_name"].as_str() else {
        error!("No release found");
        return Err("No release found".into());
    };

    let date = response["created_at"].as_str().map(String::from).unwrap();

    info!(
        "Found release with tag {} ({})",
        tag,
        format_asset_date(&date)
    );

    if binary_path.exists() {
        let metadata = std::fs::metadata(&binary_path)?;
        let local_datetime = OffsetDateTime::from(metadata.modified()?);

        info!(
            "Local binary found at {} ({})",
            binary_path.display(),
            format_date(local_datetime)
        );

        if let Ok(release_datetime) = OffsetDateTime::parse(&date, &Rfc3339) {
            if local_datetime >= release_datetime {
                info!("Binary is up to date. Nothing to do.",);
                return Ok(());
            }

            info!(
                "Local binary is older ({}) than latest asset ({}). Replacing.",
                format_date(local_datetime),
                format_date(release_datetime)
            );
        } else {
            warn!(
                "Could not parse asset date {}; assuming it is newer and replacing",
                date
            );
        }
    } else {
        info!("Local binary not found. Installing.")
    }

    let mut compatible_name = None;
    let mut compatible_url = None;

    if let Some(assets) = response["assets"].as_array() {
        for asset in assets {
            let name = match asset["name"].as_str() {
                Some(n) => n,
                None => continue,
            };

            let name_lower = name.to_lowercase();

            // Test 1: Must match the current OS
            if !os_matches(&name_lower, OS) {
                continue;
            }

            // Test 2: Must match the current architecture
            if !arch_matches(&name_lower, ARCH) {
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

            compatible_name = asset["name"].as_str().map(String::from);
            compatible_url = asset["browser_download_url"].as_str().map(String::from);
            break;
        }
    }

    let (Some(name), Some(url)) = (compatible_name, compatible_url) else {
        error!("No {OS}/{ARCH} asset found in release");
        return Err(format!("No {OS}/{ARCH} asset found in release").into());
    };

    info!("Found {OS}/{ARCH} asset {}", name);

    let tmp_dir = tempfile::Builder::new().prefix("bini-").tempdir()?;
    let tmp_download_path = tmp_dir.path().join(&name);
    info!("Created temporary folder {}", tmp_dir.path().display());

    info!("Downloading asset into {}", tmp_download_path.display());
    let mut response = reqwest::blocking::get(url)?;
    let mut out_file = File::create(&tmp_download_path)?;
    copy(&mut response, &mut out_file)?;

    if name.ends_with(".tar.gz") || name.ends_with(".tgz") || name.ends_with(".gz") {
        info!("Extracting gzip archive...");
        let tar_gz = File::open(tmp_download_path)?;
        let tar = flate2::read::GzDecoder::new(tar_gz);
        let mut archive = tar::Archive::new(tar);
        archive.unpack(&tmp_dir)?;
    } else if name.ends_with(".zip") {
        info!("Extracting zip archive...");
        let file = File::open(tmp_download_path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        archive.extract(&tmp_dir)?;
    } else {
        info!("Assuming this is an executable");
        make_executable(&tmp_download_path)?;
    }

    let executable = find_executable(&tmp_dir.path())
        .ok_or("No executable file found in the downloaded asset.")?;
    info!(
        "Found executable: {}",
        executable.file_name().unwrap().to_string_lossy()
    );

    let installation_path = installation_directory.join(binary_name);
    fs::copy(&executable, &installation_path)?;
    info!("Installed executable into {}", installation_path.display());

    if let Some(state_dir) = state_dir() {
        let index_path = state_dir.join("bini/index.txt");
        match record_installation(&index_path, binary_name, package) {
            Ok(()) => info!("Recorded {} in the install index", binary_name),
            Err(e) => warn!(
                "Failed to record installation in {}: {}",
                index_path.display(),
                e
            ),
        }
    } else {
        warn!("Could not determine state dir; skipping install index");
    }

    info!("Removed temporary folder {}", tmp_dir.path().display());

    Ok(())
}

fn list_binaries(installation_directory: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let format = format_description!("[year]-[month]-[day]");
    let mut binaries: Vec<(String, String)> = Vec::new();

    for entry in fs::read_dir(installation_directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().into_owned();
        let modified = entry.metadata()?.modified()?;
        let date = OffsetDateTime::from(modified).format(format)?;
        binaries.push((name, date));
    }

    binaries.sort();

    for (name, date) in binaries {
        println!("{} ({})", name, date);
    }

    Ok(())
}

/// Appends `binary_name,package` to the index file, replacing any existing
/// line for the same binary so the index keeps one entry per installed binary.
fn record_installation(index_path: &Path, binary_name: &str, package: &str) -> std::io::Result<()> {
    let mut lines: Vec<String> = Vec::new();

    if index_path.exists() {
        lines = fs::read_to_string(index_path)?
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with(&format!("{},", binary_name)))
            .map(str::to_string)
            .collect();
    }

    lines.push(format!("{},{}", binary_name, package));

    if let Some(parent) = index_path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(index_path, lines.join("\n") + "\n")
}

/// Updates every binary listed in the install index by re-installing it from
/// its recorded source, keeping each binary's recorded name (`--as` included).
fn update_binaries(installation_directory: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let index_path = state_dir().ok_or("No state dir")?.join("bini/index.txt");

    if !index_path.exists() {
        info!(
            "No install index found at {}; nothing to update",
            index_path.display()
        );
        return Ok(());
    }

    let contents = fs::read_to_string(&index_path)?;
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let Some((binary_name, package)) = line.split_once(',') else {
            warn!("Skipping malformed index line: {line}");
            continue;
        };

        info!("Updating {} from {}", binary_name, package);
        if let Err(e) = install(package, Some(binary_name), installation_directory) {
            warn!("Failed to update {} ({}): {}", binary_name, package, e);
        }
    }

    Ok(())
}

/// Removes an installed binary and its entry in the install index.
fn remove_binary(
    binary_name: &str,
    installation_directory: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let binary_path = installation_directory.join(binary_name);

    match fs::remove_file(&binary_path) {
        Ok(()) => info!("Removed {}", binary_path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => warn!(
            "Binary {} not found in {}",
            binary_name,
            installation_directory.display()
        ),
        Err(e) => return Err(e.into()),
    }

    if let Some(state_dir) = state_dir() {
        let index_path = state_dir.join("bini/index.txt");
        match remove_from_index(&index_path, binary_name) {
            Ok(true) => info!("Removed {} from the install index", binary_name),
            Ok(false) => {}
            Err(e) => warn!("Failed to update index {}: {}", index_path.display(), e),
        }
    } else {
        warn!("Could not determine state dir; skipping index update");
    }

    Ok(())
}

/// Removes the index line for `binary_name`. Returns true if a line was removed.
fn remove_from_index(index_path: &Path, binary_name: &str) -> std::io::Result<bool> {
    if !index_path.exists() {
        return Ok(false);
    }

    let contents = fs::read_to_string(index_path)?;
    let remaining: Vec<&str> = contents
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with(&format!("{},", binary_name)))
        .collect();

    if remaining.len() == contents.lines().count() {
        return Ok(false);
    }

    fs::write(index_path, remaining.join("\n") + "\n")?;
    Ok(true)
}
