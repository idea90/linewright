//! Keeping Linewright up to date: finding a newer release on GitHub, downloading its setup and checking it arrived
//! whole, and running it. The window (`ui::updates`) decides when; nothing here shows anything.
//!
//! Every release carries a setup for each system and a SHA256SUMS file. A Linewright installed by the setup is
//! updated in place by running the new setup with `--quiet`; one run from anywhere else opens the new setup's
//! window, to install it.

use std::cmp::Ordering;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

/// The version running now.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const RELEASES: &str = "https://api.github.com/repos/idea90/linewright/releases?per_page=20";

/// Where the list of releases is read from: GitHub, or for trying updates out, the address in LINEWRIGHT_UPDATE_URL
/// (a file of GitHub's JSON served from anywhere).
pub fn releases_url() -> String {
    std::env::var("LINEWRIGHT_UPDATE_URL").unwrap_or_else(|_| RELEASES.to_string())
}

/// The page that lists every release, for someone to look at.
pub const RELEASES_PAGE: &str = "https://github.com/idea90/linewright/releases";

/// The setup a release carries for this system.
pub const SETUP_ASSET: &str = if cfg!(windows) { "linewright-setup-windows-x86_64.exe" } else { "linewright-setup-linux-x86_64" };

const SUMS_ASSET: &str = "SHA256SUMS";

/// A setup is a few tens of megabytes; anything far larger is not one.
const LARGEST_SETUP: u64 = 512 * 1024 * 1024;

/// A release newer than the one running.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    /// Its page on GitHub, with what changed.
    pub page: String,
    pub setup_url: String,
    pub sums_url: String,
}

/// Compare two versions such as 1.0.0-alpha.6.3: the numbers before the hyphen, then a release without a
/// pre-release part above one with it, then the pre-release parts one by one (numbers as numbers, words as words,
/// and a longer list above a shorter one it starts with). A leading `v` is ignored.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let split = |version: &str| {
        let version = version.trim().trim_start_matches(['v', 'V']);
        let version = version.split('+').next().unwrap_or(version);
        match version.split_once('-') {
            Some((core, pre)) => (core.to_string(), Some(pre.to_string())),
            None => (version.to_string(), None),
        }
    };
    let ((core_a, pre_a), (core_b, pre_b)) = (split(a), split(b));
    let numbers = |core: &str| core.split('.').map(|part| part.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    let (mut numbers_a, mut numbers_b) = (numbers(&core_a), numbers(&core_b));
    let length = numbers_a.len().max(numbers_b.len());
    numbers_a.resize(length, 0);
    numbers_b.resize(length, 0);
    numbers_a.cmp(&numbers_b).then_with(|| match (pre_a, pre_b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) => {
            let (parts_a, parts_b): (Vec<&str>, Vec<&str>) = (a.split('.').collect(), b.split('.').collect());
            for (x, y) in parts_a.iter().zip(&parts_b) {
                let order = match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(x), Ok(y)) => x.cmp(&y),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => x.cmp(y),
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
            parts_a.len().cmp(&parts_b.len())
        }
    })
}

/// The newest release above `current` that carries a setup for this system, from GitHub's list of releases as
/// JSON. Drafts are passed over; pre-releases count, since every release so far is one.
pub fn newest_release(json: &str, current: &str) -> Option<Release> {
    let releases: serde_json::Value = serde_json::from_str(json).ok()?;
    releases
        .as_array()?
        .iter()
        .filter(|release| !release["draft"].as_bool().unwrap_or(false))
        .filter_map(|release| {
            let version = release["tag_name"].as_str()?.trim_start_matches(['v', 'V']).to_string();
            let asset = |name: &str| {
                release["assets"].as_array()?.iter().find(|asset| asset["name"].as_str() == Some(name))?["browser_download_url"]
                    .as_str()
                    .map(str::to_string)
            };
            Some(Release {
                page: release["html_url"].as_str().unwrap_or(RELEASES_PAGE).to_string(),
                setup_url: asset(SETUP_ASSET)?,
                sums_url: asset(SUMS_ASSET)?,
                version,
            })
        })
        .filter(|release| compare_versions(&release.version, current) == Ordering::Greater)
        .max_by(|a, b| compare_versions(&a.version, &b.version))
}

fn agent() -> ureq::Agent {
    // The system's own certificates, so a company's proxy that the system trusts is trusted here too.
    let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .user_agent(format!("Linewright/{VERSION}"))
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_per_read(Some(Duration::from_secs(30)))
        .build()
        .into()
}

/// What went wrong reaching GitHub, in words for the person waiting.
fn unreachable(error: ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(403 | 429) => "GitHub is asking for a pause between checks. Try again in an hour.".into(),
        ureq::Error::StatusCode(code) => format!("GitHub answered with an error ({code})."),
        other => format!("GitHub could not be reached: {other}"),
    }
}

/// Ask GitHub for a release newer than this one.
pub fn check() -> Result<Option<Release>, String> {
    let mut response = agent().get(releases_url()).header("Accept", "application/vnd.github+json").call().map_err(unreachable)?;
    let json = response.body_mut().read_to_string().map_err(unreachable)?;
    Ok(newest_release(&json, VERSION))
}

/// The checksum SHA256SUMS gives for a file, as lowercase hex.
pub fn checksum_for(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (sum, name) = line.trim().split_once(char::is_whitespace)?;
        (name.trim().trim_start_matches('*') == file).then(|| sum.to_lowercase())
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Download a release's setup into a folder of its own in the system's temporary folder, telling `progress` the
/// share done so far (0 to 1, when the size is known). The setup is kept only if its checksum is the one the
/// release gives for it.
pub fn download(release: &Release, progress: &mut dyn FnMut(f32)) -> Result<PathBuf, String> {
    let agent = agent();
    let mut sums = agent.get(&release.sums_url).call().map_err(unreachable)?;
    let sums = sums.body_mut().read_to_string().map_err(unreachable)?;
    let expected = checksum_for(&sums, SETUP_ASSET).ok_or("The release does not give the setup's checksum.")?;

    let mut response = agent.get(&release.setup_url).call().map_err(unreachable)?;
    let size = response.body().content_length().filter(|size| *size > 0);
    if size.is_some_and(|size| size > LARGEST_SETUP) {
        return Err("The download is far larger than a setup.".into());
    }
    let folder = std::env::temp_dir().join(format!("linewright-update-{}", release.version));
    std::fs::create_dir_all(&folder).map_err(|e| format!("The update cannot be saved: {e}"))?;
    let path = folder.join(SETUP_ASSET);
    let mut file = std::fs::File::create(&path).map_err(|e| format!("The update cannot be saved: {e}"))?;
    let mut reader = response.body_mut().with_config().limit(LARGEST_SETUP).reader();
    let (mut hasher, mut done, mut buffer) = (Sha256::new(), 0u64, vec![0u8; 64 * 1024]);
    loop {
        let read = reader.read(&mut buffer).map_err(|e| format!("The download stopped: {e}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read]).map_err(|e| format!("The update cannot be saved: {e}"))?;
        done += read as u64;
        if let Some(size) = size {
            progress((done as f32 / size as f32).min(1.0));
        }
    }
    file.flush().map_err(|e| format!("The update cannot be saved: {e}"))?;
    drop(file);
    if hex(&hasher.finalize()) != expected {
        let _ = std::fs::remove_file(&path);
        return Err("The download is not the setup the release gave: its checksum differs. Try again.".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).map_err(|e| format!("The update cannot be run: {e}"))?;
    }
    Ok(path)
}

/// The folder Linewright was installed in by the setup, when the running program is that installation: the setup
/// leaves its record beside the program.
pub fn installed_folder() -> Option<PathBuf> {
    let program = std::env::current_exe().ok()?.canonicalize().ok()?;
    let folder = program.parent()?;
    folder.join("installed.txt").is_file().then(|| folder.to_path_buf())
}

/// Install the downloaded setup over the installation in `folder`, with no window, and wait for it. The running
/// program can be replaced while it runs; the new one starts next time.
pub fn install_quietly(setup: &Path, folder: &Path) -> Result<(), String> {
    let output = std::process::Command::new(setup)
        .arg("--quiet")
        .arg(format!("--folder={}", folder.display()))
        .output()
        .map_err(|e| format!("The setup could not be started: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&output.stderr);
    let last = said.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("").trim();
    Err(if last.is_empty() { "The setup could not install the update.".into() } else { last.to_string() })
}

/// Open the downloaded setup's window, for a Linewright that was not installed by the setup.
pub fn open_setup(setup: &Path) -> Result<(), String> {
    std::process::Command::new(setup).spawn().map(|_| ()).map_err(|e| format!("The setup could not be started: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_their_numbers_and_pre_releases_say() {
        use Ordering::*;
        assert_eq!(compare_versions("1.0.0-alpha.6.3", "1.0.0-alpha.6.2"), Greater);
        assert_eq!(compare_versions("1.0.0-alpha.6.10", "1.0.0-alpha.6.9"), Greater, "numbers as numbers");
        assert_eq!(compare_versions("1.0.0-alpha.6.1", "1.0.0-alpha.6"), Greater, "a longer list above its start");
        assert_eq!(compare_versions("1.0.0-beta", "1.0.0-alpha.9"), Greater);
        assert_eq!(compare_versions("1.0.0", "1.0.0-rc.1"), Greater, "a release above its pre-releases");
        assert_eq!(compare_versions("v1.0.1", "1.0.0"), Greater);
        assert_eq!(compare_versions("1.0", "1.0.0"), Equal);
        assert_eq!(compare_versions("1.0.0-alpha.6.3", "v1.0.0-alpha.6.3"), Equal);
        assert_eq!(compare_versions("0.9.9", "1.0.0-alpha"), Less);
    }

    fn release(tag: &str, draft: bool, setup: bool) -> serde_json::Value {
        let mut assets = vec![serde_json::json!({"name": "SHA256SUMS", "browser_download_url": format!("https://x/{tag}/SHA256SUMS")})];
        if setup {
            assets.push(serde_json::json!({"name": SETUP_ASSET, "browser_download_url": format!("https://x/{tag}/setup")}));
        }
        serde_json::json!({"tag_name": tag, "draft": draft, "html_url": format!("https://x/{tag}"), "assets": assets})
    }

    #[test]
    fn the_newest_release_with_a_setup_is_offered() {
        let json = serde_json::json!([
            release("v1.0.0-alpha.7", true, true),
            release("v1.0.0-alpha.6.5", false, false),
            release("v1.0.0-alpha.6.4", false, true),
            release("v1.0.0-alpha.6.3", false, true),
        ])
        .to_string();
        let newest = newest_release(&json, "1.0.0-alpha.6.3").expect("6.4 is newer");
        assert_eq!(newest.version, "1.0.0-alpha.6.4", "the draft and the release without a setup are passed over");
        assert_eq!(newest.setup_url, "https://x/v1.0.0-alpha.6.4/setup");
        assert_eq!(newest.page, "https://x/v1.0.0-alpha.6.4");
        assert_eq!(newest_release(&json, "1.0.0-alpha.6.4"), None, "nothing newer");
        assert_eq!(newest_release("not json", "1.0.0"), None);
    }

    #[test]
    fn a_checksum_is_found_by_its_file_name() {
        let sums = "abc123  linewright-linux-x86_64\nDEF456 *linewright-setup-linux-x86_64\n";
        assert_eq!(checksum_for(sums, "linewright-setup-linux-x86_64").as_deref(), Some("def456"));
        assert_eq!(checksum_for(sums, "linewright-linux-x86_64").as_deref(), Some("abc123"));
        assert_eq!(checksum_for(sums, "other"), None);
    }

    /// Asks GitHub itself: `cargo test -- --ignored the_releases_on_github_can_be_read`.
    #[test]
    #[ignore = "reaches GitHub"]
    fn the_releases_on_github_can_be_read() {
        let found = check().expect("GitHub answers");
        println!("newer than {VERSION}: {found:?}");
    }
}
