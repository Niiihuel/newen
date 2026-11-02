//! A release's "what's new" (docs/UPDATES.md): `whats-new.json`, which the signed manifest
//! names by size and SHA-256, holds the release's notes (New, Improved, Fixed) and maybe an
//! announcement whose media it names the same way. It is downloaded with the update, and
//! nothing is kept that does not match what was named: a medium that does not is left out and
//! the words still show.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{disk, fetch, hex, sha256, usable, via, Artifact, Error, Release, Source, Version};

/// Its name among the release's files.
pub const FILE: &str = "whats-new.json";
/// No `whats-new.json` is larger.
pub const MAX_JSON: u64 = 256 * 1024;
/// No medium is larger: a short clip, or a screenshot.
pub const MAX_MEDIUM: u64 = 16 << 20;

/// What a release says is new in it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WhatsNew {
    pub version: Version,
    /// When it came out, as the author wrote it (`2026-11-10`).
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub notes: Notes,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub announcement: Option<Announcement>,
}

/// A release's notes in their three groups, each a list of short lines (a little Markdown).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Notes {
    pub new: Vec<String>,
    pub improved: Vec<String>,
    pub fixed: Vec<String>,
}

/// The one thing a release puts forward, with a picture or a clip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Announcement {
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub bullets: Vec<String>,
    /// In order of preference: quill shows the first that downloads and matches.
    #[serde(default)]
    pub media: Vec<Medium>,
    /// Where "Try it" takes the person, from the list the interface knows; one it does not
    /// know shows no button.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub try_it: Option<String>,
}

/// A picture or a clip, by address, size and digest; a clip names the picture it shows while
/// it cannot play.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Medium {
    pub kind: Kind,
    #[serde(flatten)]
    pub file: Artifact,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster: Option<Artifact>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Image,
    Video,
    /// A kind a later release may add: skipped.
    #[serde(other)]
    Other,
}

/// A medium on this computer, matched against its digest.
#[derive(Clone, Debug, PartialEq)]
pub struct Kept {
    pub kind: Kind,
    pub file: PathBuf,
    /// A clip's picture.
    pub poster: Option<PathBuf>,
}

/// `release`'s what's new, downloaded and matched against the signed entry, with the first of
/// its announcement's media that matches too, kept in `folder` under its digest.
pub fn fetch_whats_new(
    agent: &ureq::Agent,
    source: &Source,
    release: &Release,
    folder: &Path,
) -> Result<(WhatsNew, Option<Kept>), Error> {
    let entry = release.whats_new.as_ref().ok_or(Error::NoArtifact)?;
    let bytes = get(agent, source, release, entry, MAX_JSON)?;
    let whats_new: WhatsNew = serde_json::from_slice(&bytes).map_err(|error| {
        eprintln!("quill: update: unreadable {FILE}: {error}");
        Error::Manifest
    })?;
    if whats_new.version != release.version {
        return Err(Error::Manifest);
    }
    std::fs::create_dir_all(folder).map_err(disk)?;
    let media = whats_new.announcement.iter().flat_map(|a| &a.media);
    let kept = media
        .filter(|medium| medium.kind != Kind::Other)
        .find_map(|medium| {
            let kept = keep(agent, source, release, &medium.file, folder).and_then(|file| {
                let poster = match (&medium.kind, &medium.poster) {
                    (Kind::Video, Some(poster)) => {
                        Some(keep(agent, source, release, poster, folder)?)
                    }
                    // A clip shows its picture until it plays: it needs one.
                    (Kind::Video, None) => return Err(Error::Manifest),
                    _ => None,
                };
                Ok(Kept {
                    kind: medium.kind,
                    file,
                    poster,
                })
            });
            kept.inspect_err(|error| {
                eprintln!("quill: update: a medium of what's new was left out: {error}")
            })
            .ok()
        });
    Ok((whats_new, kept))
}

/// The bytes of `file`, once they are its size and digest.
fn get(
    agent: &ureq::Agent,
    source: &Source,
    release: &Release,
    file: &Artifact,
    limit: u64,
) -> Result<Vec<u8>, Error> {
    if !usable(file) || file.size > limit {
        return Err(Error::TooLarge);
    }
    let url = via(&release.assets, &file.url);
    let bytes = fetch(agent, &url, source.grant(), file.size as usize)?;
    if bytes.len() as u64 != file.size
        || hex(&Sha256::digest(&bytes)) != file.sha256.to_ascii_lowercase()
    {
        return Err(Error::Digest);
    }
    Ok(bytes)
}

/// `file` in `folder`, named by its digest: downloaded unless an earlier try left it whole.
fn keep(
    agent: &ureq::Agent,
    source: &Source,
    release: &Release,
    file: &Artifact,
    folder: &Path,
) -> Result<PathBuf, Error> {
    let digest = file.sha256.to_ascii_lowercase();
    let path = folder.join(format!("{digest}.{}", extension(&file.url)));
    if sha256(&path).is_ok_and(|found| found == digest) {
        return Ok(path);
    }
    let bytes = get(agent, source, release, file, MAX_MEDIUM)?;
    let part = path.with_extension("part");
    std::fs::write(&part, bytes).map_err(disk)?;
    std::fs::rename(&part, &path).map_err(disk)?;
    Ok(path)
}

/// The extension of the file `url` names, if it is a short plain one.
fn extension(url: &str) -> String {
    let name = url.rsplit('/').next().unwrap_or_default();
    match name.rsplit_once('.') {
        Some((_, ext))
            if (1..=8).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_alphanumeric()) =>
        {
            ext.to_ascii_lowercase()
        }
        _ => "bin".into(),
    }
}
