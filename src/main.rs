use clap::{Parser, Subcommand};
use dirs;
use env_logger::{Builder, Env};
use log::{error, info, warn};
use serde_json;
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
    after_help = "Examples:\n  bini install bootandy/dust\n  bini i burntsushi/ripgrep --as rg\n  bini i ahmetb/kubectx --match kubens --as kns\n  bini i openfaas/faas-cli --match '^faas-cli$'"
)]
struct Args {
    /// The name of the package to install
    #[arg(value_parser = sanitize_name)]
    name: Option<String>,

    /// Install the binary under a different name
    #[arg(long = "as", requires = "name")]
    as_name: Option<String>,

    /// Override up-to-date binary
    #[arg(short, long)]
    force: bool,

    /// Only consider assets whose name matches this string or regex
    #[arg(short, long = "match", requires = "name", value_parser = parse_match)]
    match_str: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Install from GitHub repository
    #[command(
        visible_aliases = ["i"],
        after_help = "Examples:\n  bini install bootandy/dust\n  bini i burntsushi/ripgrep --as rg\n  bini i ahmetb/kubectx --match kubens --as kns\n  bini i openfaas/faas-cli --match '^faas-cli$'"
    )]
    Install {
        /// The name of the package to install
        #[arg(value_parser = sanitize_name)]
        name: String,

        /// Install the binary under a different name
        #[arg(long = "as", requires = "name")]
        as_name: Option<String>,

        /// Override up-to-date binary
        #[arg(short, long, requires = "name")]
        force: bool,

        /// Only consider assets whose name matches this string or regex
        #[arg(short, long, requires = "name", value_parser = parse_match)]
        match_str: Option<String>,
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
        /// Override up-to-date binaries
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

fn parse_match(s: &str) -> Result<String, String> {
    if is_regex_like(s) {
        regex::Regex::new(s).map_err(|e| format!("invalid regular expression: {e}"))?;
    }

    Ok(s.to_string())
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

/// Returns true if a --match value should be treated as a regular expression
/// rather than a plain string, i.e. it contains at least one character
/// outside letters, digits, '-', '_', '.', and spaces.
fn is_regex_like(pattern: &str) -> bool {
    pattern
        .chars()
        .any(|c| !c.is_alphanumeric() && !matches!(c, '-' | '_' | '.' | ' '))
}

/// Selects the release asset that matches the current OS, architecture and match_filter.
/// If match_filter is a regular expression, os and arch tests ar skipped, and first match is returned
fn get_compatible_asset(
    assets: &[serde_json::Value],
    match_filter: Option<&str>,
) -> Option<(String, String)> {
    // A regex-like filter is compiled once, anchored so that it only
    // matches a whole asset name. An invalid pattern (only reachable from
    // a hand-edited index entry replayed by update) selects nothing, like
    // a pattern with no match, and the caller reports an error.
    let full_re = match match_filter.filter(|m| is_regex_like(m)) {
        Some(m) => Some(regex::Regex::new(&format!("^(?:{m})$")).ok()?),
        _ => None,
    };

    let mut best: Option<(String, String)> = None;
    let mut best_len = usize::MAX;

    for asset in assets {
        let Some(name) = asset["name"].as_str() else {
            continue;
        };
        let Some(url) = asset["browser_download_url"].as_str() else {
            continue;
        };

        let name_lower = name.to_lowercase();

        // Test 0: a regex-like --match must fully match the asset name; the
        // first match is returned directly, bypassing the tests below. A
        // name that does not match is skipped entirely, so a filter with no
        // match at all leaves `best` empty and the caller reports an error.
        if let Some(re) = &full_re {
            if re.is_match(name) {
                return Some((name.to_string(), url.to_string()));
            }
            continue;
        }

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

        // Test 4: If --match is set, the asset name must contain the match string
        if let Some(match_str) = match_filter {
            if !name_lower.contains(&match_str.to_lowercase()) {
                continue;
            }
        }

        // Among all that pass, keep the asset with the shortest name
        if name.len() < best_len {
            best_len = name.len();
            best = Some((name.to_string(), url.to_string()));
        }
    }

    best
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

fn display_tilde(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);

    if let Some(home_path) = home {
        if let Ok(rel) = path.strip_prefix(home_path) {
            return format!("~/{}", rel.display());
        }
    }
    path.display().to_string()
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
        warn!(
            "Created installation directory {}",
            display_tilde(&installation_directory),
        );
    }
    if !is_in_path(&installation_directory) {
        warn!(
            "Consider adding {} to your PATH",
            display_tilde(&installation_directory),
        )
    }

    // setup index path
    let state_dir = dirs::state_dir().unwrap_or(data_dir).join("bini");
    if !state_dir.exists() {
        std::fs::create_dir_all(&state_dir)?;
        warn!("Created state directory {}", display_tilde(&state_dir));
    }
    let index_path = state_dir.join("index.txt");

    let args = Args::parse();

    let _ = match args.command {
        Some(Command::List { version }) => {
            let binaries = list_binaries(&installation_directory, &index_path)?;
            print_binaries(&binaries, version);
            return Ok(());
        }
        Some(Command::Install {
            name,
            as_name,
            force,
            match_str,
        }) => {
            return install(
                name,
                as_name,
                &installation_directory,
                &index_path,
                force,
                match_str.as_deref(),
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
                    name,
                    args.as_name,
                    &installation_directory,
                    &index_path,
                    args.force,
                    args.match_str.as_deref(),
                );
            } else {
                return update_binaries(&installation_directory, &index_path, args.force);
            }
        }
    };
}

fn install(
    package: String,
    as_name: Option<String>,
    installation_directory: &Path,
    index_path: &Path,
    force: bool,
    match_str: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo_name = package.split('/').last().unwrap();
    let binary_name = as_name.unwrap_or(repo_name.to_string());

    let repo_buf;
    let repo_str = match &match_str {
        Some(m) => {
            repo_buf = format!("{repo_name}:{m}");
            &repo_buf
        }
        _ => repo_name,
    };

    let log_target_buf;
    let log_target = if binary_name == repo_name {
        repo_str
    } else {
        log_target_buf = format!("{repo_str}({binary_name})");
        &log_target_buf
    };

    info!(target: &log_target, "Installing {} as {}", package, binary_name);

    let url = format!("https://api.github.com/repos/{}/releases/latest", package);
    let client = reqwest::blocking::Client::new();
    info!(target: &log_target, "Checking GitHub's latest release of {}", package);

    let response = client
        .get(&url)
        .header("User-Agent", "bini")
        .send()?
        .json::<serde_json::Value>()?;

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

    let binary_path = installation_directory.join(&binary_name);
    if binary_path.exists() {
        let metadata = std::fs::metadata(&binary_path)?;
        let local_datetime = time::OffsetDateTime::from(metadata.modified()?);

        info!(
            target: &log_target,
            "Local binary found at {} ({})",
            display_tilde(&binary_path),
            format_date(local_datetime),
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
        warn!(target: &log_target, "Local binary not found. Installing.")
    }

    let Some((name, url)) = response["assets"]
        .as_array()
        .and_then(|a| get_compatible_asset(a, match_str))
    else {
        let match_msg = match_str
            .map(|m| format!(" matching \"{m}\""))
            .unwrap_or_default();
        error!(target: &log_target, "No {OS}/{ARCH} asset{match_msg} found in release");
        return Err(format!("No {OS}/{ARCH} asset{match_msg} found in release").into());
    };

    info!(target: &log_target, "Found {OS}/{ARCH} asset {}", name);

    let tmp_dir = tempfile::Builder::new().prefix("bini-").tempdir()?;
    let tmp_download_path = tmp_dir.path().join(&name);
    info!(target: &log_target, "Created temporary folder {}", tmp_dir.path().display());

    info!(target: &log_target, "Downloading asset into {}", tmp_download_path.display());
    let mut response = reqwest::blocking::get(url)?;
    let mut out_file = std::fs::File::create(&tmp_download_path)?;
    copy(&mut response, &mut out_file)?;
    drop(out_file);

    if name.ends_with(".tar.gz") || name.ends_with(".tgz") || name.ends_with(".gz") {
        info!(target: &log_target, "Extracting gzip archive...");
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

    let installation_path = installation_directory.join(&binary_name);
    let current_exe = env::current_exe()?;

    // handle self-update
    // TODO: as_name / binary_path cannot be bini (or only if package = lapwat/bini)
    // otherwise bini will be replaced by another binary
    if installation_path == current_exe {
        self_replace::self_replace(&executable)?;
    } else {
        std::fs::copy(&executable, &installation_path)?;
    }

    // set modification date to release date
    let mtime = filetime::FileTime::from_unix_time(
        release_datetime.unix_timestamp(),
        release_datetime.nanosecond(),
    );

    filetime::set_file_mtime(&installation_path, mtime)?;

    info!(target: &log_target, "Installed executable into {}", display_tilde(&installation_path));

    match record_installation(&index_path, &binary_name, &package, match_str) {
        Ok(()) => info!(target: &log_target, "Recorded {} in the install index", binary_name),
        Err(e) => warn!(
            target: &log_target,
            "Failed to record installation in {}: {}",
            display_tilde(&index_path),
            e,
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

/// Looks up the source package recorded for `binary_name` in the install
/// index. Returns `None` if the index is missing or the binary has no entry.
fn get_installation_record(
    index_path: &Path,
    binary_name: &str,
) -> Option<(String, Option<String>)> {
    let contents = fs::read_to_string(index_path).ok()?;
    for line in contents.lines() {
        let line = line.trim();

        // check line
        if line.is_empty() {
            continue;
        }

        let mut parts = line.splitn(3, ',');

        // check name
        let Some(name) = parts.next() else {
            continue;
        };
        if name != binary_name {
            continue;
        }

        let Some(package) = parts.next() else {
            continue;
        };

        let match_str = parts.next().filter(|s| !s.is_empty()).map(str::to_string);

        return Some((package.to_string(), match_str));
    }
    None
}

/// An installed binary tracked by bini, with its recorded source package,
/// install date, optional `--match` filter, and location on disk.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct InstalledBinary {
    name: String,
    package: String,
    date: String,
    path: PathBuf,
    match_str: Option<String>,
}

/// Collects the binaries installed in `installation_directory`, enriched with
/// their recorded source package and install date. The result is sorted by
/// binary name (then package, date, and path to break ties).
fn list_binaries(
    installation_directory: &Path,
    index_path: &Path,
) -> Result<Vec<InstalledBinary>, Box<dyn std::error::Error>> {
    let format = format_description!("[year]-[month]-[day]");
    let mut binaries: Vec<InstalledBinary> = Vec::new();

    for entry in fs::read_dir(installation_directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().into_owned();
        let modified = entry.metadata()?.modified()?;
        let date = time::OffsetDateTime::from(modified).format(format)?;

        let Some((package, match_str)) = get_installation_record(index_path, &name) else {
            warn!("Binary {} not found in index", name);
            continue;
        };

        binaries.push(InstalledBinary {
            name,
            package,
            date,
            path: entry.path(),
            match_str,
        });
    }

    binaries.sort();

    Ok(binaries)
}

/// Prints installed binaries. When `version` is set, each binary's detected
/// version is included by querying the executable directly.
fn print_binaries(binaries: &[InstalledBinary], version: bool) {
    for binary in binaries {
        let pkg_str = match &binary.match_str {
            Some(m) => format!("{}:{}", binary.package, m),
            None => binary.package.to_string(),
        };

        let line = if version {
            match get_executable_version(&binary.path) {
                Some(v) => format!("{} {} from {} ({})", binary.name, v, pkg_str, binary.date),
                None => format!("{} from {} ({})", binary.name, pkg_str, binary.date),
            }
        } else {
            format!("{} from {} ({})", binary.name, pkg_str, binary.date)
        };
        println!("{}", line);
    }
}

/// Appends `binary_name,package[,match]` to the index file, replacing any
/// existing line for the same binary so the index keeps one entry per
/// installed binary. The optional `match` field records a `--match` filter.
fn record_installation(
    index_path: &Path,
    binary_name: &str,
    package: &str,
    match_filter: Option<&str>,
) -> std::io::Result<()> {
    let mut lines: Vec<String> = fs::read_to_string(index_path)?
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with(&format!("{},", binary_name)))
        .map(str::to_string)
        .collect();

    let entry = match match_filter {
        Some(m) => format!("{},{},{}", binary_name, package, m),
        _ => format!("{},{}", binary_name, package),
    };
    lines.push(entry);

    fs::write(index_path, lines.join("\n") + "\n")
}

/// Updates every binary listed in the install index by re-installing it from
/// its recorded source, keeping each binary's recorded name (`--as` included)
/// and `--match` filter.
fn update_binaries(
    installation_directory: &Path,
    index_path: &Path,
    force: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let binaries = list_binaries(installation_directory, index_path)?;
    for binary in binaries {
        if let Err(e) = install(
            binary.package.clone(),
            Some(binary.name.clone()),
            installation_directory,
            index_path,
            force,
            binary.match_str.as_deref(),
        ) {
            warn!(
                "Failed to update {} ({}): {}",
                binary.name, binary.package, e
            );
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
        Ok(()) => info!("Removed {}", display_tilde(&binary_path)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => warn!(
            "Binary {} not found in {}",
            binary_name,
            display_tilde(&installation_directory),
        ),
        Err(e) => return Err(e.into()),
    }

    match remove_from_index(&index_path, binary_name) {
        Ok(true) => info!("Removed {} from the install index", binary_name),
        Ok(false) => {}
        Err(e) => warn!(
            "Failed to update index {}: {}",
            display_tilde(&index_path),
            e,
        ),
    }

    Ok(())
}

/// Removes the index line for `binary_name`. Returns true if a line was removed.
fn remove_from_index(index_path: &Path, binary_name: &str) -> std::io::Result<bool> {
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
