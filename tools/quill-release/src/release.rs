//! The release side of quill's updates, for the owner's machine and the
//! release workflow (`docs/UPDATES.md`):
//!
//! ```text
//! quill-release keygen <private-key-file>   a new key: the private half to the file, the public to stdout
//! quill-release tag <tag>                   the tag is annotated and names this workspace's version
//! quill-release whats-new <tag> <base-url> <out-dir>
//!                                          whats-new.json and its media from release/notes/<version>.toml
//!                                          and release/media/<version>/, into <out-dir>; prints each file
//! quill-release manifest <tag> <base-url> [--whats-new <file>] <platform>=<file>...
//!                                          the manifest of the release, to stdout
//! quill-release sign <manifest>             <manifest>.sig, with QUILL_UPDATE_SIGNING_KEY
//! quill-release verify <manifest>           <manifest>.sig verifies with the key compiled in, and the
//!                                          whats-new.json and media beside it match what it names
//! ```
//!
//! The private key is 32 bytes in base64 and never printed.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use base64::Engine as _;
use ed25519_dalek::{Signer, SigningKey};
use quill_update::whats_new::{self, Announcement, Kind, Medium, Notes, WhatsNew};
use quill_update::{Artifact, Manifest, Version};
use serde::Deserialize;
use sha2::{Digest, Sha256};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let result = match arguments.as_slice() {
        ["keygen", file] => keygen(Path::new(file)),
        ["tag", tag] => tag_version(tag).map(|version| println!("{version}")),
        ["whats-new", tag, base, out] => build_whats_new_for(tag, base, Path::new(out)),
        ["manifest", tag, base, "--whats-new", notes, artifacts @ ..] if !artifacts.is_empty() => {
            manifest(tag, base, Some(Path::new(notes)), artifacts)
        }
        ["manifest", tag, base, artifacts @ ..] if !artifacts.is_empty() => {
            manifest(tag, base, None, artifacts)
        }
        ["sign", file] => sign(Path::new(file)),
        ["verify", file] => verify(Path::new(file)),
        _ => Err("usage: quill-release keygen <file> | tag <tag> | whats-new <tag> <base-url> <out-dir> | manifest <tag> <base-url> [--whats-new <file>] <platform>=<file>... | sign <manifest> | verify <manifest>".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("quill-release: {error}");
            ExitCode::FAILURE
        }
    }
}

type Result<T> = std::result::Result<T, String>;

fn base64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn keygen(file: &Path) -> Result<()> {
    let mut seed = [0; 32];
    getrandom::fill(&mut seed).map_err(|error| error.to_string())?;
    let key = SigningKey::from_bytes(&seed);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut out = options
        .open(file)
        .map_err(|error| format!("{}: {error}", file.display()))?;
    writeln!(out, "{}", base64().encode(seed)).map_err(|error| error.to_string())?;
    println!("{}", base64().encode(key.verifying_key().to_bytes()));
    Ok(())
}

fn git(arguments: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(arguments)
        .output()
        .map_err(|error| format!("git: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {}: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| error.to_string())
}

/// Every annotated tag: its name and message.
fn annotated_tags() -> Result<Vec<(String, String)>> {
    let listed = git(&[
        "for-each-ref",
        "refs/tags",
        "--format=%(refname:strip=2)%00%(objecttype)%00%(contents)%00",
    ])?;
    let fields: Vec<&str> = listed.split('\0').collect();
    Ok(fields
        .chunks_exact(3)
        .filter(|tag| tag[1] == "tag")
        .map(|tag| (tag[0].trim().to_owned(), tag[2].trim().to_owned()))
        .collect())
}

/// The version `tag` names, once it is annotated and names this
/// workspace's version: a release from a forgotten bump would offer
/// itself again forever.
fn tag_version(tag: &str) -> Result<Version> {
    if !annotated_tags()?.iter().any(|(name, _)| name == tag) {
        return Err(format!(
            "{tag} is not an annotated tag: git tag -a {tag} -m \"…\""
        ));
    }
    let version = Version::parse(tag.strip_prefix('v').ok_or("a tag starts with v")?)
        .map_err(|error| format!("{tag}: {error}"))?;
    if version.to_string() != quill_update::VERSION {
        return Err(format!(
            "{tag} does not match the workspace's version {}",
            quill_update::VERSION
        ));
    }
    Ok(version)
}

fn tag_date(tag: &str) -> Result<String> {
    Ok(git(&[
        "for-each-ref",
        &format!("refs/tags/{tag}"),
        "--format=%(taggerdate:iso-strict)",
    ])?
    .trim()
    .to_owned())
}

/// `file` as the release serves it from `base`: its address, size and digest.
fn served(base: &str, file: &Path, name: &str) -> Result<Artifact> {
    let bytes = std::fs::read(file).map_err(|error| format!("{}: {error}", file.display()))?;
    Ok(Artifact {
        url: format!("{}/{name}", base.trim_end_matches('/')),
        size: bytes.len() as u64,
        sha256: quill_update::hex(&Sha256::digest(&bytes)),
    })
}

fn file_name(file: &Path) -> Result<&str> {
    file.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("{}: no file name", file.display()))
}

fn manifest(tag: &str, base: &str, notes: Option<&Path>, artifacts: &[&str]) -> Result<()> {
    let version = tag_version(tag)?;
    let tags = annotated_tags()?;
    let tag_notes = tags
        .iter()
        .find(|(name, _)| name == tag)
        .map(|(_, message)| message.clone())
        .unwrap_or_default();
    let minimum_version = quill_update::minimum_version(
        &version,
        tags.iter()
            .map(|(name, message)| (name.as_str(), message.as_str())),
    );
    let date = tag_date(tag)?;
    let mut platforms = std::collections::BTreeMap::new();
    for artifact in artifacts {
        let (platform, file) = artifact
            .split_once('=')
            .ok_or_else(|| format!("{artifact}: <platform>=<file>"))?;
        let file = Path::new(file);
        platforms.insert(platform.to_owned(), served(base, file, file_name(file)?)?);
    }
    let whats_new = match notes {
        Some(file) => Some(served(base, file, whats_new::FILE)?),
        None => None,
    };
    let manifest = Manifest {
        version,
        date,
        notes: tag_notes,
        minimum_version,
        platforms,
        whats_new,
        assets: Default::default(),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&manifest).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn signature_path(file: &Path) -> std::path::PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".sig");
    name.into()
}

fn sign(file: &Path) -> Result<()> {
    let seed = std::env::var("QUILL_UPDATE_SIGNING_KEY")
        .map_err(|_| "QUILL_UPDATE_SIGNING_KEY is not set")?;
    let seed: [u8; 32] = base64()
        .decode(seed.trim())
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or("QUILL_UPDATE_SIGNING_KEY is not a key from `quill-release keygen`")?;
    let key = SigningKey::from_bytes(&seed);
    let bytes = std::fs::read(file).map_err(|error| format!("{}: {error}", file.display()))?;
    // Only what quill would accept is signed.
    serde_json::from_slice::<Manifest>(&bytes)
        .map_err(|error| format!("not a manifest: {error}"))?;
    let signature = base64().encode(key.sign(&bytes).to_bytes());
    std::fs::write(signature_path(file), format!("{signature}\n"))
        .map_err(|error| error.to_string())
}

fn verify(file: &Path) -> Result<()> {
    let key = quill_update::release_key()
        .ok_or("no release key compiled in: run quill-release keygen and commit crates/quill-update/release-key.pub")?;
    let bytes = std::fs::read(file).map_err(|error| format!("{}: {error}", file.display()))?;
    let signature = std::fs::read_to_string(signature_path(file))
        .map_err(|error| format!("{}.sig: {error}", file.display()))?;
    let manifest = quill_update::verify(&bytes, &signature, &key).map_err(|error| {
        format!("{error:?}: it does not verify with release-key.pub (another signing secret, or a changed manifest)")
    })?;
    let folder = file.parent().unwrap_or(Path::new("."));
    if let Some(media) = check_whats_new(&manifest, folder)? {
        println!("{} verified ({media} media)", whats_new::FILE);
    }
    println!(
        "{} verified (minimum {})",
        manifest.version,
        manifest
            .minimum_version
            .map_or("none".into(), |minimum| minimum.to_string())
    );
    Ok(())
}

/// What a release's notes say, as their author writes them in `release/notes/<version>.toml`
/// (docs/UPDATES.md); the media's files are in `release/media/<version>/`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Written {
    /// The tag's date when left out.
    date: Option<String>,
    #[serde(default)]
    new: Vec<String>,
    #[serde(default)]
    improved: Vec<String>,
    #[serde(default)]
    fixed: Vec<String>,
    announcement: Option<WrittenAnnouncement>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenAnnouncement {
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    bullets: Vec<String>,
    try_it: Option<String>,
    #[serde(default)]
    media: Vec<WrittenMedium>,
}

/// A picture, or a clip with the picture it shows until it plays.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenMedium {
    file: String,
    poster: Option<String>,
}

/// The longest a title, a body and a line may be, as quill shows them, and the most points.
const TITLE: usize = 60;
const BODY: usize = 280;
const LINE: usize = 160;
const BULLETS: usize = 5;

fn build_whats_new_for(tag: &str, base: &str, out: &Path) -> Result<()> {
    let version = tag_version(tag)?;
    let date = tag_date(tag)?;
    let notes = PathBuf::from(format!("release/notes/{version}.toml"));
    let media = PathBuf::from(format!("release/media/{version}"));
    for file in build_whats_new(&version, &date, &notes, &media, base, out)? {
        println!("{}", file.display());
    }
    Ok(())
}

/// `whats-new.json` for `version` from the notes written in `notes` and the media in `media`,
/// each medium copied into `out` under the name it is served by, beside it. The files written,
/// `whats-new.json` first. A line too long or a medium too large stops it: quill would cut it.
fn build_whats_new(
    version: &Version,
    date: &str,
    notes: &Path,
    media: &Path,
    base: &str,
    out: &Path,
) -> Result<Vec<PathBuf>> {
    let text =
        std::fs::read_to_string(notes).map_err(|error| format!("{}: {error}", notes.display()))?;
    let written: Written =
        toml::from_str(&text).map_err(|error| format!("{}: {error}", notes.display()))?;
    let long = |what: &str, text: &str, most: usize| {
        (text.chars().count() > most)
            .then(|| format!("{what} is longer than {most} characters: {text}"))
    };
    let lines = (written.new.iter())
        .chain(&written.improved)
        .chain(&written.fixed);
    if let Some(error) = lines.filter_map(|line| long("a note", line, LINE)).next() {
        return Err(error);
    }
    std::fs::create_dir_all(out).map_err(|error| format!("{}: {error}", out.display()))?;
    let mut files = vec![out.join(whats_new::FILE)];
    let mut copy = |name: &str| -> Result<Artifact> {
        let from = media.join(name);
        let served_as = format!("whats-new-{name}");
        if !served_as
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(format!(
                "{name}: a medium's name is letters, digits, '.', '_' and '-'"
            ));
        }
        let to = out.join(&served_as);
        std::fs::copy(&from, &to).map_err(|error| format!("{}: {error}", from.display()))?;
        let file = served(base, &to, &served_as)?;
        if file.size > whats_new::MAX_MEDIUM {
            return Err(format!(
                "{name} is larger than {} bytes",
                whats_new::MAX_MEDIUM
            ));
        }
        files.push(to);
        Ok(file)
    };
    let announcement = match written.announcement {
        Some(a) => {
            let too_long = [
                long("the title", &a.title, TITLE),
                long("the body", &a.body, BODY),
                (a.bullets.len() > BULLETS).then(|| format!("more than {BULLETS} points")),
            ];
            let points = a.bullets.iter().filter_map(|b| long("a point", b, LINE));
            if let Some(error) = too_long.into_iter().flatten().chain(points).next() {
                return Err(error);
            }
            let mut media = vec![];
            for medium in &a.media {
                media.push(Medium {
                    kind: if medium.poster.is_some() {
                        Kind::Video
                    } else {
                        Kind::Image
                    },
                    file: copy(&medium.file)?,
                    poster: medium.poster.as_deref().map(&mut copy).transpose()?,
                });
            }
            Some(Announcement {
                title: a.title,
                body: a.body,
                bullets: a.bullets,
                media,
                try_it: a.try_it,
            })
        }
        None => None,
    };
    let whats_new = WhatsNew {
        version: version.clone(),
        // The tag's date is RFC 3339: its day.
        date: written
            .date
            .unwrap_or_else(|| date.chars().take(10).collect()),
        notes: Notes {
            new: written.new,
            improved: written.improved,
            fixed: written.fixed,
        },
        announcement,
    };
    let json = serde_json::to_vec_pretty(&whats_new).map_err(|error| error.to_string())?;
    std::fs::write(&files[0], json).map_err(|error| error.to_string())?;
    Ok(files)
}

/// The `whats-new.json` `manifest` names and every medium it names, beside it in `folder`
/// under their served names, are the size and digest they are said to be; how many media.
fn check_whats_new(manifest: &Manifest, folder: &Path) -> Result<Option<usize>> {
    let Some(entry) = &manifest.whats_new else {
        return Ok(None);
    };
    let matches = |file: &Artifact| -> Result<Vec<u8>> {
        let name = file.url.rsplit('/').next().unwrap_or_default();
        let path = folder.join(name);
        let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        if bytes.len() as u64 != file.size
            || quill_update::hex(&Sha256::digest(&bytes)) != file.sha256
        {
            return Err(format!("{name} is not the file the release names"));
        }
        Ok(bytes)
    };
    let notes: WhatsNew = serde_json::from_slice(&matches(entry)?)
        .map_err(|error| format!("{}: {error}", whats_new::FILE))?;
    if notes.version != manifest.version {
        return Err(format!("{} is {}'s", whats_new::FILE, notes.version));
    }
    let media = notes.announcement.iter().flat_map(|a| &a.media);
    let mut count = 0;
    for medium in media {
        matches(&medium.file)?;
        if let Some(poster) = &medium.poster {
            matches(poster)?;
        }
        count += 1;
    }
    Ok(Some(count))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The notes of a release with a picture and a clip become its whats-new.json, the
    /// manifest names it and the signature covers it; verify's check finds every file, and a
    /// medium changed afterwards is caught.
    #[test]
    fn a_release_with_whats_new_is_built_signed_and_checked() {
        let repo = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let media = repo.path().join("media");
        std::fs::create_dir_all(&media).unwrap();
        std::fs::write(media.join("browser.png"), b"a picture").unwrap();
        std::fs::write(media.join("browser.mp4"), b"a clip").unwrap();
        let notes = repo.path().join("0.2.0.toml");
        std::fs::write(
            &notes,
            r#"
new = ["A browser beside the chat"]
fixed = ["The strip no longer flickers"]

[announcement]
title = "A browser beside the chat"
body = "Open any page next to the session."
try_it = "browser"

[[announcement.media]]
file = "browser.mp4"
poster = "browser.png"

[[announcement.media]]
file = "browser.png"
"#,
        )
        .unwrap();
        let version = Version::parse("0.2.0").unwrap();
        let base = "https://github.com/Niiihuel/quill/releases/download/v0.2.0";
        let files = build_whats_new(
            &version,
            "2026-11-10T12:00:00-03:00",
            &notes,
            &media,
            base,
            out.path(),
        )
        .unwrap();
        assert_eq!(files.len(), 4, "{files:?}");
        let built: WhatsNew = serde_json::from_slice(&std::fs::read(&files[0]).unwrap()).unwrap();
        assert_eq!(built.date, "2026-11-10");
        let a = built.announcement.unwrap();
        assert_eq!(
            (a.media[0].kind, a.media[1].kind),
            (Kind::Video, Kind::Image)
        );
        assert_eq!(a.media[0].file.url, format!("{base}/whats-new-browser.mp4"));

        let manifest = Manifest {
            version: version.clone(),
            date: "2026-11-10T12:00:00-03:00".into(),
            notes: String::new(),
            minimum_version: None,
            platforms: Default::default(),
            whats_new: Some(served(base, &files[0], whats_new::FILE).unwrap()),
            assets: Default::default(),
        };
        let bytes = serde_json::to_vec_pretty(&manifest).unwrap();
        let key = SigningKey::from_bytes(&[7; 32]);
        let signature = base64().encode(key.sign(&bytes).to_bytes());
        let read = quill_update::verify(&bytes, &signature, &key.verifying_key()).unwrap();
        assert_eq!(read.whats_new, manifest.whats_new);
        assert_eq!(check_whats_new(&read, out.path()), Ok(Some(2)));
        std::fs::write(out.path().join("whats-new-browser.png"), b"another").unwrap();
        assert!(check_whats_new(&read, out.path()).is_err());
        // A title quill would cut stops the release instead.
        std::fs::write(
            &notes,
            format!("[announcement]\ntitle = \"{}\"\n", "x".repeat(61)),
        )
        .unwrap();
        assert!(build_whats_new(&version, "", &notes, &media, base, out.path()).is_err());
    }
}
