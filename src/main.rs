use clap::{Parser, Subcommand};
use dirs;
use env_logger::{Builder, Env};
use log::{error, info, warn};
use serde_json::Value;
use std::env;
use std::env::consts::{ARCH, OS};
use std::fs;
use std::io::copy;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
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

    /// Force binary replacement
    #[arg(short, long)]
    force: bool,

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
        #[arg(long = "as", requires = "name")]
        as_name: Option<String>,

        /// Force binary replacement
        #[arg(short, long, requires = "name")]
        force: bool,
    },

    /// List installed binaries
    #[command(visible_aliases = ["l"])]
    List {
        /// Show binary version, trying --version, version, -v, -V
        #[arg(short, long)]
        version: bool,
    },

    /// Update all installed binaries
    #[command(visible_aliases = ["u"], after_help = "Examples:\n  bini update\n  bini")]
    Update {
        /// Force binary replacement
        #[arg(short, long)]
        force: bool,
    },

    /// Remove installed binary
    #[command(
        visible_aliases = ["r", "rm", "uninstall", "delete"],
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
        _ => return false,
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

/// Formats an `time::OffsetDateTime` as YYYY-MM-DD.
fn format_date(datetime: time::OffsetDateTime) -> String {
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
    match time::OffsetDateTime::parse(date, &Rfc3339) {
        Ok(datetime) => format_date(datetime),
        Err(_) => date.to_string(),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // configure logs
    Builder::from_env(Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    // setup installation directory
    let data_dir = dirs::data_dir().ok_or("Failed to evaluate data directory")?;
    let installation_directory = data_dir.join("bini/bin");
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

    // setup index path
    let state_dir = dirs::state_dir().unwrap_or(data_dir).join("bini");
    if !state_dir.exists() {
        std::fs::create_dir_all(&state_dir)?;
        info!("Created state directory {}", state_dir.display());
    }
    let index_path = state_dir.join("index.txt");

    let args = Args::parse();

    let _ = match args.command {
        Some(Command::List { version }) => return list_binaries(&installation_directory, version),
        Some(Command::Install {
            name,
            as_name,
            force,
        }) => {
            return install(
                &name,
                as_name.as_deref(),
                &installation_directory,
                &index_path,
                force,
            );
        }
        Some(Command::Update { force }) => {
            return update_binaries(&installation_directory, &index_path, force);
        }
        Some(Command::Remove { name }) => {
            return remove_binary(&name, &installation_directory, &index_path);
        }
        _ => {
            if let Some(name) = args.name {
                return install(
                    &name,
                    args.as_name.as_deref(),
                    &installation_directory,
                    &index_path,
                    args.force,
                );
            } else {
                return update_binaries(&installation_directory, &index_path, args.force);
            }
        }
    };
}

fn install(
    package: &str,
    as_name: Option<&str>,
    installation_directory: &Path,
    index_path: &Path,
    force: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let binary_name = match as_name {
        Some(alias) => alias,
        _ => package.split('/').last().unwrap(),
    };
    let binary_path = installation_directory.join(binary_name);
    let log_target = format!("bini {package}");

    info!(target: &log_target, "Installing {} as {}", package, binary_name);

    let url = format!("https://api.github.com/repos/{}/releases/latest", package);
    let client = reqwest::blocking::Client::new();
    info!(target: &log_target, "Checking GitHub's latest release at {}", url);

    let response = client
        .get(&url)
        .header("User-Agent", "bini")
        .send()?
        .json::<Value>()?;

    let Some(tag) = response["tag_name"].as_str() else {
        error!(target: &log_target, "No release found");
        return Err("No release found".into());
    };

    let date = response["created_at"].as_str().map(String::from).unwrap();
    let release_datetime = time::OffsetDateTime::parse(&date, &Rfc3339)?;

    info!(
        target: &log_target,
        "Found release with tag {} ({})",
        tag,
        format_asset_date(&date)
    );

    if binary_path.exists() {
        let metadata = std::fs::metadata(&binary_path)?;
        let local_datetime = time::OffsetDateTime::from(metadata.modified()?);

        info!(
            target: &log_target,
            "Local binary found at {} ({})",
            binary_path.display(),
            format_date(local_datetime)
        );

        if local_datetime >= release_datetime {
            if force {
                warn!(target: &log_target, "Binary is up to date. Replacing anyway.");
            } else {
                info!(target: &log_target, "Binary is up to date. Nothing to do.");
                return Ok(());
            }
        } else {
            warn!(
                target: &log_target,
                "Local binary is older ({}) than latest asset ({}). Replacing.",
                format_date(local_datetime),
                format_date(release_datetime)
            );
        }
    } else {
        info!(target: &log_target, "Local binary not found. Installing.")
    }

    let mut compatible_name = None;
    let mut compatible_url = None;

    if let Some(assets) = response["assets"].as_array() {
        for asset in assets {
            let name = match asset["name"].as_str() {
                Some(n) => n,
                _ => continue,
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
        error!(target: &log_target, "No {OS}/{ARCH} asset found in release");
        return Err(format!("No {OS}/{ARCH} asset found in release").into());
    };

    info!(target: &log_target, "Found {OS}/{ARCH} asset {}", name);

    let tmp_dir = tempfile::Builder::new().prefix("bini").tempdir()?;
    let tmp_download_path = tmp_dir.path().join(&name);
    info!(target: &log_target, "Created temporary folder {}", tmp_dir.path().display());

    info!(target: &log_target, "Downloading asset into {}", tmp_download_path.display());
    let mut response = reqwest::blocking::get(url)?;
    let mut out_file = std::fs::File::create(&tmp_download_path)?;
    copy(&mut response, &mut out_file)?;
    drop(out_file);

    if name.ends_with(".tar.gz") || name.ends_with(".tgz") || name.ends_with(".gz") {
        // info!(target: &log_target, "Extracting gzip archive...");
        let tar_gz = std::fs::File::open(tmp_download_path)?;
        let tar = flate2::read::GzDecoder::new(tar_gz);
        let mut archive = tar::Archive::new(tar);
        archive.unpack(&tmp_dir)?;
    } else if name.ends_with(".zip") {
        info!(target: &log_target, "Extracting zip archive...");
        let file = std::fs::File::open(tmp_download_path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        archive.extract(&tmp_dir)?;
    } else {
        info!(target: &log_target, "Assuming this is an executable");
        make_executable(&tmp_download_path)?;
    }

    let executable = find_executable(&tmp_dir.path())
        .ok_or("No executable file found in the downloaded asset.")?;
    info!(
        target: &log_target,
        "Found executable: {}",
        executable.file_name().unwrap().to_string_lossy()
    );

    let installation_path = installation_directory.join(binary_name);
    let in_install_dir = env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .and_then(|p| p.parent().map(|p| p == installation_directory))
        .unwrap_or(false);

    // handle self-update and running bini from bin directory
    if package == "lapwat/bini" && in_install_dir {
        self_replace::self_replace(&executable)?;
        std::fs::remove_file(&executable)?;
    } else {
        std::fs::copy(&executable, &installation_path)?;
    }

    // set modification date to release date
    let mtime = filetime::FileTime::from_unix_time(
        release_datetime.unix_timestamp(),
        release_datetime.nanosecond(),
    );

    filetime::set_file_mtime(&installation_path, mtime)?;

    info!(target: &log_target, "Installed executable into {}", installation_path.display());

    match record_installation(&index_path, binary_name, package) {
        Ok(()) => info!(target: &log_target, "Recorded {} in the install index", binary_name),
        Err(e) => warn!(
            target: &log_target,
            "Failed to record installation in {}: {}",
            index_path.display(),
            e
        ),
    }

    info!(target: &log_target, "Removed temporary folder {}", tmp_dir.path().display());

    Ok(())
}

fn get_executable_version(executable_path: &Path) -> Option<String> {
    let re = regex::Regex::new(r"v?(\d+\.\d+\.\d+)").ok()?;
    let args_list = ["--version", "version", "-v", "-V"];

    for arg in args_list {
        let output = match std::process::Command::new(executable_path)
            .arg(arg)
            .output()
        {
            Ok(o) => o,
            Err(_) => continue,
        };

        let text = if !output.stdout.is_empty() {
            String::from_utf8_lossy(&output.stdout).into_owned()
        } else if !output.stderr.is_empty() {
            String::from_utf8_lossy(&output.stderr).into_owned()
        } else {
            continue;
        };

        if let Some(caps) = re.captures(&text) {
            if let Some(m) = caps.get(1) {
                return Some(format!("v{}", m.as_str()));
            }
        }
    }

    None
}

fn list_binaries(
    installation_directory: &Path,
    version: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let format = format_description!("[year]-[month]-[day]");
    let mut binaries: Vec<(String, String, PathBuf)> = Vec::new();

    for entry in fs::read_dir(installation_directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().into_owned();
        let modified = entry.metadata()?.modified()?;
        let date = time::OffsetDateTime::from(modified).format(format)?;
        binaries.push((name, date, entry.path()));
    }

    binaries.sort();

    for (name, date, path) in binaries {
        if version {
            match get_executable_version(&path) {
                Some(v) => println!("{} {} ({})", name, v, date),
                None => println!("{} ({})", name, date),
            }
        } else {
            println!("{} ({})", name, date);
        }
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
fn update_binaries(
    installation_directory: &Path,
    index_path: &Path,
    force: bool,
) -> Result<(), Box<dyn std::error::Error>> {
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

        if let Err(e) = install(
            package,
            Some(binary_name),
            installation_directory,
            index_path,
            force,
        ) {
            warn!("Failed to update {} ({}): {}", binary_name, package, e);
        }
    }

    Ok(())
}

/// Removes an installed binary and its entry in the install index.
fn remove_binary(
    binary_name: &str,
    installation_directory: &Path,
    index_path: &Path,
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

    match remove_from_index(&index_path, binary_name) {
        Ok(true) => info!("Removed {} from the install index", binary_name),
        Ok(false) => {}
        Err(e) => warn!("Failed to update index {}: {}", index_path.display(), e),
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
