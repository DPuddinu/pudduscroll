use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use serde::Deserialize;

use crate::library::library_directory;

const LATEST: &str = "https://api.github.com/repos/DPuddinu/pudduscroll/releases/latest";
const MAX_BYTES: u64 = 80 * 1024 * 1024;
const DOWNLOAD_PREFIX: &str = "https://github.com/DPuddinu/pudduscroll/releases/download/";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Offer {
    pub(crate) version: String,
    pub(crate) notes: String,
    tag: String,
    asset: String,
}

#[derive(Clone, Debug)]
pub(crate) enum Event {
    Available(Offer),
    Installing(String),
    Restart,
    Failed(String),
}

#[must_use]
pub(crate) fn start() -> Option<Receiver<Event>> {
    if cfg!(debug_assertions) {
        return None;
    }
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || check(&tx));
    Some(rx)
}

#[must_use]
pub(crate) fn apply(offer: Offer) -> Receiver<Event> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(Event::Installing(offer.version.clone()));
        match install_offer(&offer) {
            Ok(()) => {
                let _ = tx.send(Event::Restart);
            }
            Err(error) => {
                let _ = tx.send(Event::Failed(error));
            }
        }
    });
    rx
}

fn check(tx: &Sender<Event>) {
    let Ok(body) = fetch_bytes(
        LATEST,
        Duration::from_secs(20),
        "application/vnd.github+json",
    ) else {
        return;
    };
    let Ok(text) = String::from_utf8(body) else {
        return;
    };
    match plan(
        &text,
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        read_attempt(),
    ) {
        Step::Quiet => {}
        Step::UpToDate => clear_attempt(),
        Step::Tell(message) => {
            let _ = tx.send(Event::Failed(message));
        }
        Step::Offer(offer) => {
            let _ = tx.send(Event::Available(offer));
        }
    }
}

fn install_offer(offer: &Offer) -> Result<(), String> {
    let Some(version) = Version::parse(&offer.version) else {
        return Err("Could not read the update version".to_string());
    };
    install(version, &offer.tag, &offer.asset)
}

fn install(version: Version, tag: &str, asset: &str) -> Result<(), String> {
    let current = installed_path()?;
    ensure_writable(&current)?;
    let archive = fetch_bytes(
        &download_url(tag, asset),
        Duration::from_secs(180),
        "application/octet-stream",
    )?;
    let binary = extract_binary(asset, &archive)?;
    let staged = write_staged(&binary)?;
    if let Err(error) = write_attempt(version) {
        let _ = std::fs::remove_file(&staged);
        return Err(error);
    }
    if let Err(error) = spawn_replacer(&current, &staged) {
        clear_attempt();
        let _ = std::fs::remove_file(&staged);
        return Err(error);
    }
    Ok(())
}

fn installed_path() -> Result<PathBuf, String> {
    let current = std::env::current_exe()
        .map_err(|error| format!("Could not find the installed app: {error}"))?;
    #[cfg(unix)]
    {
        Ok(std::fs::canonicalize(&current).unwrap_or(current))
    }
    #[cfg(not(unix))]
    {
        Ok(current)
    }
}

fn ensure_writable(current: &Path) -> Result<(), String> {
    let parent = current
        .parent()
        .ok_or_else(|| "Could not find the installed app".to_string())?;
    let probe = parent.join(format!(".pudduscroll-{}", std::process::id()));
    std::fs::write(&probe, b"ok")
        .map_err(|error| format!("Could not write the update into the app folder: {error}"))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

fn write_staged(bytes: &[u8]) -> Result<PathBuf, String> {
    let name = if cfg!(windows) {
        format!("pudduscroll-{}.exe", std::process::id())
    } else {
        format!("pudduscroll-{}", std::process::id())
    };
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, bytes).map_err(|error| format!("Could not save the update: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .map_err(|error| format!("Could not save the update: {error}"))?;
    };
    Ok(path)
}

fn spawn_replacer(current: &Path, staged: &Path) -> Result<(), String> {
    let script = script_path();
    std::fs::write(&script, replacer_script())
        .map_err(|error| format!("Could not start the update: {error}"))?;
    launch_replacer(current, staged, &script)
}

fn script_path() -> PathBuf {
    let ext = if cfg!(windows) { "ps1" } else { "sh" };
    std::env::temp_dir().join(format!("pudduscroll-update-{}.{ext}", std::process::id()))
}

const fn replacer_script() -> &'static str {
    if cfg!(windows) {
        WINDOWS_SCRIPT
    } else {
        UNIX_SCRIPT
    }
}

fn launch_replacer(current: &Path, staged: &Path, script: &Path) -> Result<(), String> {
    let mut command = replacer_command(script);
    command
        .env("PUDDU_NEW", staged)
        .env("PUDDU_OLD", current)
        .env("PUDDU_PID", std::process::id().to_string())
        .env("PUDDU_SCRIPT", script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    detach(&mut command);
    #[cfg(windows)]
    hide_and_detach(&mut command);
    command
        .spawn()
        .map_err(|error| format!("Could not start the update: {error}"))?;
    Ok(())
}

fn replacer_command(script: &Path) -> Command {
    if cfg!(windows) {
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ]);
        command.arg(script);
        command
    } else {
        let mut command = Command::new("sh");
        command.arg(script);
        command
    }
}

#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    let _ = command.process_group(0);
}

#[cfg(windows)]
fn hide_and_detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    let _ = command.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
}

const WINDOWS_SCRIPT: &str = r"$ErrorActionPreference = 'Stop'
$deadline = (Get-Date).AddMinutes(2)
do {
  try {
    Move-Item -LiteralPath $env:PUDDU_NEW -Destination $env:PUDDU_OLD -Force
    break
  } catch {
    if ((Get-Date) -ge $deadline) { exit 1 }
    Start-Sleep -Milliseconds 300
  }
} while ($true)
Start-Process -FilePath $env:PUDDU_OLD
Remove-Item -LiteralPath $env:PUDDU_SCRIPT -Force -ErrorAction SilentlyContinue
";

const UNIX_SCRIPT: &str = r#"#!/bin/sh
set -u
i=0
while kill -0 "$PUDDU_PID" 2>/dev/null; do
  i=$((i + 1))
  if [ "$i" -gt 300 ]; then
    exit 1
  fi
  sleep 0.2
done
mv -f "$PUDDU_NEW" "$PUDDU_OLD" || exit 1
chmod 755 "$PUDDU_OLD" || exit 1
nohup "$PUDDU_OLD" >/dev/null 2>&1 &
rm -f "$PUDDU_SCRIPT"
"#;

fn attempt_path() -> PathBuf {
    library_directory().join("update-tag")
}

fn read_attempt() -> Option<Version> {
    let text = std::fs::read_to_string(attempt_path()).ok()?;
    Version::parse(text.trim())
}

fn write_attempt(version: Version) -> Result<(), String> {
    let dir = library_directory();
    std::fs::create_dir_all(&dir).map_err(|error| format!("Could not save the update: {error}"))?;
    std::fs::write(attempt_path(), version.to_string())
        .map_err(|error| format!("Could not save the update: {error}"))
}

fn clear_attempt() {
    let _ = std::fs::remove_file(attempt_path());
}

fn fetch_bytes(url: &str, timeout: Duration, accept: &str) -> Result<Vec<u8>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout(timeout)
        .build();
    let response = agent
        .get(url)
        .set(
            "User-Agent",
            concat!("pudduscroll/", env!("CARGO_PKG_VERSION")),
        )
        .set("Accept", accept)
        .call()
        .map_err(|error| format!("Could not reach GitHub: {error}"))?;
    if let Some(length) = response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok())
        && length > MAX_BYTES
    {
        return Err("The update is larger than 80 MB".to_string());
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not download the update: {error}"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("The update is larger than 80 MB".to_string());
    }
    if bytes.is_empty() {
        return Err("The update download was empty".to_string());
    }
    Ok(bytes)
}

fn download_url(tag: &str, asset: &str) -> String {
    format!("{DOWNLOAD_PREFIX}{tag}/{asset}")
}

#[derive(Debug, Deserialize)]
struct ReleaseBody {
    tag_name: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

impl Version {
    fn parse(text: &str) -> Option<Self> {
        let text = text.trim().strip_prefix('v').unwrap_or(text.trim());
        let mut parts = text.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Step {
    Quiet,
    UpToDate,
    Tell(String),
    Offer(Offer),
}

fn plan(body: &str, installed: &str, os: &str, arch: &str, tried: Option<Version>) -> Step {
    let Ok(release) = serde_json::from_str::<ReleaseBody>(body) else {
        return Step::Quiet;
    };
    if !tag_ok(&release.tag_name) {
        return Step::Quiet;
    }
    let (Some(remote), Some(running)) =
        (Version::parse(&release.tag_name), Version::parse(installed))
    else {
        return Step::Quiet;
    };
    if remote <= running {
        return Step::UpToDate;
    }
    if tried == Some(remote) {
        return Step::Tell(format!("Could not finish the update to {remote}"));
    }
    let Some(asset) = asset_name(os, arch) else {
        return Step::Quiet;
    };
    if !release.assets.iter().any(|item| item.name == asset) {
        return Step::Tell("The latest release has no build for this system".to_string());
    }
    Step::Offer(Offer {
        version: remote.to_string(),
        notes: release_notes(&release.body),
        tag: release.tag_name,
        asset: asset.to_owned(),
    })
}

fn release_notes(body: &str) -> String {
    let text = body.trim();
    if text.is_empty() {
        return String::new();
    }
    if let Some(rest) = text
        .split_once("## New")
        .map(|(_, rest)| rest.trim())
        .filter(|rest| !rest.is_empty())
    {
        return format!("## New\n{rest}");
    }
    text.to_owned()
}

fn tag_ok(tag: &str) -> bool {
    Version::parse(tag).is_some() && !tag.contains(['/', '\\', '?', '#', ' ', '%'])
}

fn asset_name(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("windows", "x86_64") => Some("pudduscroll-windows-x86_64.zip"),
        ("linux", "x86_64") => Some("pudduscroll-linux-x86_64.tar.gz"),
        ("macos", "aarch64") => Some("pudduscroll-macos-aarch64.zip"),
        ("macos", "x86_64") => Some("pudduscroll-macos-x86_64.zip"),
        _ => None,
    }
}

fn extract_binary(asset: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    match asset {
        "pudduscroll-linux-x86_64.tar.gz" => extract_tar_gz(bytes),
        "pudduscroll-windows-x86_64.zip"
        | "pudduscroll-macos-aarch64.zip"
        | "pudduscroll-macos-x86_64.zip" => extract_zip(bytes),
        _ => Err("The update archive is not a known package".to_string()),
    }
}

fn extract_zip(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("Could not read the update: {error}"))?;
    let mut best: Option<(usize, usize)> = None;
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|error| format!("Could not read the update: {error}"))?;
        if file.is_dir() {
            continue;
        }
        let Some(rank) = binary_rank(file.name()) else {
            continue;
        };
        if best.as_ref().is_some_and(|(have, _)| *have <= rank) {
            continue;
        }
        best = Some((rank, index));
    }
    let Some((_, index)) = best else {
        return Err("The update archive has no app".to_string());
    };
    let mut file = archive
        .by_index(index)
        .map_err(|error| format!("Could not read the update: {error}"))?;
    if file.size() > MAX_BYTES {
        return Err("The update is larger than 80 MB".to_string());
    }
    let data = read_capped(&mut file)?;
    if data.is_empty() {
        return Err("The update archive has no app".to_string());
    }
    Ok(data)
}

fn extract_tar_gz(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .map_err(|error| format!("Could not read the update: {error}"))?;
    let mut best: Option<(usize, Vec<u8>)> = None;
    for entry in entries {
        let mut entry = entry.map_err(|error| format!("Could not read the update: {error}"))?;
        let path = entry
            .path()
            .map_err(|error| format!("Could not read the update: {error}"))?;
        let rank = binary_rank(&path.to_string_lossy());
        let data = read_capped(&mut entry)?;
        let Some(rank) = rank else {
            continue;
        };
        if data.is_empty() || best.as_ref().is_some_and(|(have, _)| *have <= rank) {
            continue;
        }
        best = Some((rank, data));
    }
    best.map(|(_, data)| data)
        .ok_or_else(|| "The update archive has no app".to_string())
}

fn read_capped(reader: &mut impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read the update: {error}"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("The update is larger than 80 MB".to_string());
    }
    Ok(bytes)
}

fn binary_rank(name: &str) -> Option<usize> {
    if name.chars().any(|ch| ch == ':' || ch.is_control())
        || name
            .split(['/', '\\'])
            .any(|part| part.is_empty() || part == "..")
    {
        return None;
    }
    let file = name.rsplit(['/', '\\']).next()?;
    (file == "pudduscroll" || file == "pudduscroll.exe").then(|| name.split(['/', '\\']).count())
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use super::{
        Offer, Step, Version, asset_name, binary_rank, download_url, extract_binary, plan,
        release_notes, tag_ok,
    };

    fn release_json() -> &'static str {
        r#"{
            "tag_name": "v0.3.0",
            "draft": false,
            "body": "Intro\n\n## Downloads\n\ntable\n\n## New\n\n- Faster boot\n- Lazy decode\n",
            "assets": [
                {"name": "pudduscroll-windows-x86_64.zip", "browser_download_url": "https://example.invalid/win"},
                {"name": "pudduscroll-linux-x86_64.tar.gz", "browser_download_url": "https://example.invalid/linux"},
                {"name": "pudduscroll-macos-aarch64.zip", "browser_download_url": "https://example.invalid/arm"},
                {"name": "pudduscroll-macos-x86_64.zip", "browser_download_url": "https://example.invalid/intel"}
            ]
        }"#
    }

    #[test]
    fn newer_release_picks_the_asset_for_this_system() {
        let body = release_json();
        assert_eq!(
            asset_name("windows", "x86_64"),
            Some("pudduscroll-windows-x86_64.zip")
        );
        assert_eq!(
            asset_name("linux", "x86_64"),
            Some("pudduscroll-linux-x86_64.tar.gz")
        );
        assert_eq!(
            asset_name("macos", "aarch64"),
            Some("pudduscroll-macos-aarch64.zip")
        );
        assert_eq!(
            asset_name("macos", "x86_64"),
            Some("pudduscroll-macos-x86_64.zip")
        );
        assert_eq!(asset_name("windows", "aarch64"), None);
        let Step::Offer(offer) = plan(body, "0.2.0", "linux", "x86_64", None) else {
            panic!("expected an offer");
        };
        assert_eq!(
            offer,
            Offer {
                version: "0.3.0".to_string(),
                notes: "## New\n- Faster boot\n- Lazy decode".to_string(),
                tag: "v0.3.0".to_string(),
                asset: "pudduscroll-linux-x86_64.tar.gz".to_string(),
            }
        );
        assert_eq!(
            download_url(&offer.tag, &offer.asset),
            "https://github.com/DPuddinu/pudduscroll/releases/download/v0.3.0/pudduscroll-linux-x86_64.tar.gz"
        );
        assert_eq!(
            plan(body, "0.3.0", "windows", "x86_64", None),
            Step::UpToDate
        );
        assert_eq!(
            plan(body, "v0.4.0", "windows", "x86_64", None),
            Step::UpToDate
        );
        assert_eq!(plan(body, "0.2.0", "linux", "aarch64", None), Step::Quiet);
        assert!(matches!(
            plan(body, "0.2.0", "windows", "x86_64", Version::parse("0.3.0")),
            Step::Tell(_)
        ));
        assert_eq!(plan("{", "0.2.0", "windows", "x86_64", None), Step::Quiet);
        assert!(!tag_ok("v0.3.0/extra"));
        assert!(Version::parse("v0.2.0").unwrap() < Version::parse("0.2.1").unwrap());
        assert!(Version::parse("0.2.9").unwrap() < Version::parse("0.10.0").unwrap());
        assert_eq!(release_notes("plain notes"), "plain notes".to_string());
        assert_eq!(release_notes(""), String::new());
    }

    #[test]
    fn missing_asset_is_reported() {
        let body = r#"{"tag_name":"v0.3.0","assets":[{"name":"notes.txt"}]}"#;
        assert_eq!(
            plan(body, "0.2.0", "windows", "x86_64", None),
            Step::Tell("The latest release has no build for this system".to_string())
        );
    }

    #[test]
    fn archive_yields_the_app_binary() {
        let zip = zip_bytes(&[
            ("PudduScroll.app/Contents/Info.plist", b"plist"),
            ("../pudduscroll.exe", b"slip"),
            ("PudduScroll.app/Contents/MacOS/pudduscroll", b"macos-app"),
            ("pudduscroll.exe.bak", b"nope"),
        ]);
        assert_eq!(
            extract_binary("pudduscroll-macos-aarch64.zip", &zip).unwrap(),
            b"macos-app"
        );
        let both = zip_bytes(&[
            ("PudduScroll.app/Contents/MacOS/pudduscroll", b"nested"),
            ("pudduscroll.exe", b"root"),
        ]);
        assert_eq!(
            extract_binary("pudduscroll-windows-x86_64.zip", &both).unwrap(),
            b"root"
        );
        let packed = tar_gz(&[("pudduscroll", b"linux-app")]);
        assert_eq!(
            extract_binary("pudduscroll-linux-x86_64.tar.gz", &packed).unwrap(),
            b"linux-app"
        );
        let slipped = zip_bytes(&[("../pudduscroll.exe", b"slip")]);
        extract_binary("pudduscroll-windows-x86_64.zip", &slipped).unwrap_err();
        assert_eq!(binary_rank(r"foo\..\pudduscroll.exe"), None);
    }

    fn zip_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, bytes) in files {
            writer.start_file(name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn tar_gz(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, bytes) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, name, *bytes).unwrap();
        }
        let raw = builder.into_inner().unwrap();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&raw).unwrap();
        encoder.finish().unwrap()
    }
}
