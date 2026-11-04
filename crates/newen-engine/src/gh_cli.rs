//! The GitHub CLI for a computer without it: the official release from github.com/cli/cli for
//! this system, checked against the SHA-256 list published with the release, and kept in
//! Newen's tools folder, where github.rs runs it. It needs no administrator rights and is the
//! same on Linux, Windows and macOS. `gh` keeps its sign-in where it always does, so a `gh`
//! installed later by the person shares it.

use sha2::{Digest, Sha256};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const LATEST: &str = "https://api.github.com/repos/cli/cli/releases/latest";
const DOWNLOADS: &str = "https://github.com/cli/cli/releases/download";
/// No archive, checksum list or program is larger (a release's archives are about 15 MB).
const MAX_ARCHIVE: u64 = 96 << 20;
const MAX_LIST: u64 = 64 << 10;
const MAX_PROGRAM: u64 = 256 << 20;

/// Where Newen keeps its own `gh` under `tools`.
pub(crate) fn path(tools: &Path) -> PathBuf {
    let file = if cfg!(windows) { "gh.exe" } else { "gh" };
    tools.join("gh").join("bin").join(file)
}

/// The `gh` to run: the one Newen downloaded, when it did, else the system's.
pub(crate) fn program(tools: Option<&Path>) -> PathBuf {
    tools
        .map(path)
        .filter(|p| p.is_file())
        .unwrap_or_else(|| "gh".into())
}

/// This process's PATH with the folder of Newen's `gh` first, once Newen downloaded one.
pub(crate) fn on_path(tools: &Path) -> Option<std::ffi::OsString> {
    let gh = path(tools);
    let folder = gh.parent().filter(|_| gh.is_file())?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(std::iter::once(folder.to_owned()).chain(std::env::split_paths(&path)))
        .ok()
}

/// The archive of release `version` for `os` and `arch` (Rust's names), as the releases name
/// theirs; `None` for a system they do not build for.
fn archive_name(version: &str, os: &str, arch: &str) -> Option<String> {
    let arch = match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "386",
        _ => return None,
    };
    let (os, kind) = match os {
        "linux" => ("linux", "tar.gz"),
        "windows" => ("windows", "zip"),
        "macos" => ("macOS", "zip"),
        _ => return None,
    };
    Some(format!("gh_{version}_{os}_{arch}.{kind}"))
}

/// The SHA-256 the release's list gives `name` ("<sha256>  <name>" a line).
fn checksum<'a>(list: &'a str, name: &str) -> Option<&'a str> {
    list.lines().find_map(|line| {
        let (sum, file) = line.split_once(char::is_whitespace)?;
        (file.trim() == name && sum.len() == 64).then_some(sum)
    })
}

/// The program in `archive`, a .zip or else a .tar.gz: the file at `bin/gh` (`bin/gh.exe`)
/// under whatever folder the release puts it in.
fn program_in(archive: &[u8], zip: bool) -> Result<Vec<u8>, String> {
    let wanted = |name: &str| {
        let name = name.replace('\\', "/");
        name == "bin/gh"
            || name == "bin/gh.exe"
            || name.ends_with("/bin/gh")
            || name.ends_with("/bin/gh.exe")
    };
    let unreadable =
        |e: &dyn std::fmt::Display| format!("The GitHub CLI's archive is unreadable: {e}");
    let mut program = vec![];
    if zip {
        let mut files = zip::ZipArchive::new(Cursor::new(archive)).map_err(|e| unreadable(&e))?;
        for at in 0..files.len() {
            let mut file = files.by_index(at).map_err(|e| unreadable(&e))?;
            if file.is_file() && wanted(file.name()) {
                file.by_ref()
                    .take(MAX_PROGRAM + 1)
                    .read_to_end(&mut program)
                    .map_err(|e| unreadable(&e))?;
                break;
            }
        }
    } else {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
        for entry in tar.entries().map_err(|e| unreadable(&e))? {
            let mut entry = entry.map_err(|e| unreadable(&e))?;
            let name = entry
                .path()
                .map_err(|e| unreadable(&e))?
                .to_string_lossy()
                .into_owned();
            if entry.header().entry_type().is_file() && wanted(&name) {
                entry
                    .by_ref()
                    .take(MAX_PROGRAM + 1)
                    .read_to_end(&mut program)
                    .map_err(|e| unreadable(&e))?;
                break;
            }
        }
    }
    match program.len() as u64 {
        0 => Err("The GitHub CLI's archive has no gh in it.".into()),
        n if n > MAX_PROGRAM => Err("The GitHub CLI in the archive is too large.".into()),
        _ => Ok(program),
    }
}

/// Downloads the latest `gh` for this computer, checks it and keeps it under `tools`; what
/// does not match the release's checksum is never written. `step` says how far it got.
pub(crate) fn fetch(tools: &Path, step: impl Fn(String)) -> Result<PathBuf, String> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .https_only(true)
            .timeout_global(Some(Duration::from_secs(300)))
            .build(),
    );
    let get = |url: &str, limit: u64| -> Result<Vec<u8>, String> {
        let mut response = agent
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .call()
            .map_err(|e| format!("Could not reach GitHub: {e}"))?;
        (response.body_mut().with_config().limit(limit).read_to_vec())
            .map_err(|e| format!("The download did not finish: {e}"))
    };
    let latest: serde_json::Value = serde_json::from_slice(&get(LATEST, MAX_LIST)?)
        .map_err(|_| "GitHub answered something else about the GitHub CLI's releases")?;
    let version = latest["tag_name"]
        .as_str()
        .and_then(|t| t.strip_prefix('v'));
    // It goes into addresses and a file name: digits and dots only.
    let version = version
        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit() || b == b'.'))
        .ok_or("GitHub answered no version of the GitHub CLI")?;
    let name = archive_name(version, std::env::consts::OS, std::env::consts::ARCH)
        .ok_or("The GitHub CLI has no release for this computer.")?;
    let list = get(
        &format!("{DOWNLOADS}/v{version}/gh_{version}_checksums.txt"),
        MAX_LIST,
    )?;
    let list = String::from_utf8_lossy(&list);
    let sum = checksum(&list, &name)
        .ok_or("The release lists no checksum for this computer's archive.")?;
    step(format!("Downloading GitHub CLI {version}\u{2026}"));
    let archive = get(&format!("{DOWNLOADS}/v{version}/{name}"), MAX_ARCHIVE)?;
    if newen_update::hex(&Sha256::digest(&archive)) != sum.to_ascii_lowercase() {
        return Err("The download does not match the release's checksum: it was not kept.".into());
    }
    step("Unpacking\u{2026}".into());
    keep(tools, &program_in(&archive, name.ends_with(".zip"))?)
}

/// `program` as Newen's `gh` under `tools`, written beside and renamed so a half-written one is
/// never run, and executable.
fn keep(tools: &Path, program: &[u8]) -> Result<PathBuf, String> {
    let path = path(tools);
    let folder = path.parent().expect("a folder");
    let disk = |e: std::io::Error| format!("Could not keep the GitHub CLI: {e}");
    std::fs::create_dir_all(folder).map_err(disk)?;
    let part = folder.join(".gh.part");
    let mut file = std::fs::File::create(&part).map_err(disk)?;
    file.write_all(program).map_err(disk)?;
    file.sync_all().map_err(disk)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755)).map_err(disk)?;
    }
    std::fs::rename(&part, &path).map_err(disk)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_system_gets_the_archive_the_releases_build_for_it() {
        let name = |os, arch| archive_name("2.83.0", os, arch);
        assert_eq!(
            name("linux", "x86_64").as_deref(),
            Some("gh_2.83.0_linux_amd64.tar.gz")
        );
        assert_eq!(
            name("windows", "aarch64").as_deref(),
            Some("gh_2.83.0_windows_arm64.zip")
        );
        assert_eq!(
            name("macos", "aarch64").as_deref(),
            Some("gh_2.83.0_macOS_arm64.zip")
        );
        assert_eq!(name("freebsd", "x86_64"), None);
        assert_eq!(name("linux", "riscv64"), None);
    }

    #[test]
    fn the_checksum_is_the_one_listed_for_that_archive() {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let list = format!("{a}  gh_2.83.0_linux_amd64.tar.gz\n{b}  gh_2.83.0_windows_amd64.zip\n");
        assert_eq!(
            checksum(&list, "gh_2.83.0_windows_amd64.zip"),
            Some(b.as_str())
        );
        assert_eq!(checksum(&list, "gh_2.83.0_linux_arm64.tar.gz"), None);
        assert_eq!(checksum("short  gh.zip", "gh.zip"), None);
    }

    #[test]
    fn gh_comes_out_of_either_archive_and_is_kept_runnable() {
        let gh = b"#!/bin/sh\necho gh version 2.83.0\n";
        let mut gz = flate2::write::GzEncoder::new(vec![], flate2::Compression::fast());
        {
            let mut tar = tar::Builder::new(&mut gz);
            for (name, body) in [
                ("gh_2.83.0_linux_amd64/LICENSE", &b"MIT"[..]),
                ("gh_2.83.0_linux_amd64/bin/gh", gh),
            ] {
                let mut header = tar::Header::new_gnu();
                header.set_size(body.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                tar.append_data(&mut header, name, body).unwrap();
            }
            tar.finish().unwrap();
        }
        let tar_gz = gz.finish().unwrap();
        assert_eq!(program_in(&tar_gz, false).unwrap(), gh);

        let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
        let options = zip::write::FileOptions::default();
        zip.start_file("LICENSE", options).unwrap();
        zip.write_all(b"MIT").unwrap();
        zip.start_file("bin/gh.exe", options).unwrap();
        zip.write_all(gh).unwrap();
        let zip = zip.finish().unwrap().into_inner();
        assert_eq!(program_in(&zip, true).unwrap(), gh);
        assert!(program_in(&zip, false).is_err(), "a zip is not a tar.gz");

        let tools = std::env::temp_dir().join(format!("newen-gh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tools);
        assert_eq!(
            program(Some(&tools)),
            PathBuf::from("gh"),
            "the system's until one is kept"
        );
        assert_eq!(on_path(&tools), None);
        let kept = keep(&tools, gh).unwrap();
        assert_eq!(program(Some(&tools)), kept);
        let first = std::env::split_paths(&on_path(&tools).unwrap()).next();
        assert_eq!(first.as_deref(), kept.parent(), "an agent finds it first");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&kept).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
        let _ = std::fs::remove_dir_all(&tools);
    }

    /// The real download, once by hand: `cargo test -p newen-engine gh_downloads -- --ignored`.
    #[test]
    #[ignore = "downloads the GitHub CLI from github.com"]
    fn gh_downloads_checks_and_runs() {
        let tools = std::env::temp_dir().join(format!("newen-gh-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tools);
        let kept = fetch(&tools, |step| eprintln!("{step}")).unwrap();
        let out = std::process::Command::new(&kept)
            .arg("--version")
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&out.stdout).starts_with("gh version "));
        let _ = std::fs::remove_dir_all(&tools);
    }
}
