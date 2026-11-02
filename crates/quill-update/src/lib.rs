//! quill's own updates (`docs/UPDATES.md`).
//!
//! Every release publishes `quill-update.json`, a manifest of its version, notes, the oldest
//! version still accepted (`minimum_version`), one artifact per platform (address, size,
//! SHA-256) and one list of files per platform ([`files`]), and `quill-update.json.sig`, the
//! Ed25519 signature of those exact bytes in base64 (`.sigs`, more signatures, one a line, for
//! a key rotation). The public halves of the release key and of an offline backup key are
//! compiled in (`release-key.pub`, `backup-key.pub`); a manifest signed by either is trusted.
//! A manifest also names its product and channel, a `sequence` that only grows (a copy refuses
//! one lower than the highest it saw) and maybe when it expires ([`admit`]).
//!
//! A check reads the latest release of [`REPOSITORY`], which is public, at its download
//! addresses (not GitHub's API, so no token and no API rate limit); a feed at a base address
//! (`QUILL_UPDATE_FEED`, for the tests) the same way. The signature is verified
//! before the JSON is read, and an artifact is installed only once its size and digest match
//! what was signed ([`install`]). Network, disk and every wait run on the caller's thread:
//! never the interface's.

pub mod files;
pub mod install;
pub mod log;
#[cfg(any(test, feature = "serve"))]
pub mod serve;
pub mod whats_new;

use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use ed25519_dalek::Signature;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use ed25519_dalek::VerifyingKey;
pub use semver::Version;

/// This program's version: the workspace's, which a release's tag matches.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The repository the releases are published in; public, so read with no token.
pub const REPOSITORY: &str = "Niiihuel/quill";
pub const MANIFEST: &str = "quill-update.json";
pub const SIGNATURE: &str = "quill-update.json.sig";
/// More signatures of the manifest, one a line: a rotation's new keys sign here. Copies up to
/// 0.1.8 read only [`SIGNATURE`], which keeps one line.
pub const SIGNATURES: &str = "quill-update.json.sigs";
/// What every manifest of quill names as its product and channel.
pub const PRODUCT: &str = "quill";
pub const CHANNEL: &str = "stable";
/// The word in an annotated tag's message that makes its version mandatory.
pub const MANDATORY_WORD: &str = "mandatory";

const MAX_MANIFEST: usize = 64 * 1024;
const MAX_SIGNATURE: usize = 1024;
const MAX_SIGNATURES: usize = 8 * 1024;
/// No artifact is larger: a manifest that says so is refused.
pub const MAX_ARTIFACT: u64 = 1 << 30;

/// The release key's public half, base64, written by `quill-release keygen`. Empty until the
/// owner made one: then nothing is trusted, and no update is ever installed.
const PUBLIC_KEY: &str = include_str!("../release-key.pub");
/// The backup key's public half: its private half is kept offline, for when the release key is
/// lost or must be replaced (docs/UPDATES.md).
const BACKUP_KEY: &str = include_str!("../backup-key.pub");

/// What one release says of itself, signed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: Version,
    /// When it was published, RFC 3339.
    pub date: String,
    /// The annotated tag's message.
    #[serde(default)]
    pub notes: String,
    /// The newest mandatory version up to this one: anything older must update before it
    /// opens.
    #[serde(default)]
    pub minimum_version: Option<Version>,
    /// By [`platform`] key.
    pub platforms: BTreeMap<String, Artifact>,
    /// The release's `whats-new.json` ([`whats_new`]): its notes, and an announcement whose
    /// media it names by size and digest, so this signature covers them too. A copy that does
    /// not know the field ignores it, as serde does any unknown field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub whats_new: Option<Artifact>,
    /// [`PRODUCT`] and [`CHANNEL`]: a manifest of anything else is refused. Empty in a manifest
    /// from before them (0.1.8 and older).
    #[serde(default)]
    pub product: String,
    #[serde(default)]
    pub channel: String,
    /// Grows with each release; a copy refuses one lower than the highest it saw.
    #[serde(default)]
    pub sequence: u64,
    /// When it is no longer valid, RFC 3339; an expired manifest is refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
    /// The whole set of keys to trust from now on, base64: a rotation, accepted only when a key
    /// trusted now and every new key signed this manifest.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_keys: Vec<String>,
    /// By [`platform`] key, the list of the bundle's files ([`files::Bundle`]): so the
    /// signature covers it, and the manifest stays small for the copies that never read it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, Artifact>,
    /// The keys `next_keys` names when this manifest rotated them: to keep and trust from now
    /// on. Never signed: the check fills it.
    #[serde(skip)]
    pub rotated: Option<Vec<VerifyingKey>>,
}

impl Default for Manifest {
    /// An empty release 0.0.0 of quill's stable channel.
    fn default() -> Self {
        Self {
            version: Version::new(0, 0, 0),
            date: String::new(),
            notes: String::new(),
            minimum_version: None,
            platforms: BTreeMap::new(),
            whats_new: None,
            product: PRODUCT.into(),
            channel: CHANNEL.into(),
            sequence: 0,
            expires: None,
            next_keys: Vec::new(),
            files: BTreeMap::new(),
            rotated: None,
        }
    }
}

/// What a platform downloads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub url: String,
    pub size: u64,
    /// Lowercase hex.
    pub sha256: String,
}

/// A release newer than this program, as offered.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: Version,
    pub date: String,
    pub notes: String,
    /// This program is older than the release's `minimum_version`.
    pub mandatory: bool,
    /// `None`: the release has nothing for this platform.
    pub artifact: Option<Artifact>,
    /// Its `whats-new.json`, when it has a usable one.
    pub whats_new: Option<Artifact>,
    /// The list of its files for this platform ([`files`]), when it has a usable one.
    pub files: Option<Artifact>,
}

/// Why a check, a download or an installation did not go through. Each leaves quill as it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// This build has no release key: nothing can be trusted.
    NoKey,
    Network,
    Status(u16),
    TooLarge,
    Signature,
    Manifest,
    NoArtifact,
    Incomplete,
    Digest,
    Disk,
    /// This copy is not one `quill --install` placed (a build from source, say).
    NotInstalled,
    /// The manifest's sequence is lower than one seen before: an old release served again.
    Replayed,
    /// The manifest is past its `expires`.
    Expired,
    /// The manifest is another product's or channel's.
    Foreign,
    /// The job ended without saying how (its thread stopped early).
    Interrupted,
}

impl Error {
    /// What the person reads.
    pub fn label(self) -> String {
        match self {
            Self::NoKey => "This build has no update key: no update can be trusted.".into(),
            Self::Network => "Could not reach the update server.".into(),
            Self::Status(status) => format!("The update server answered {status}."),
            Self::TooLarge => "The update server's answer is too large.".into(),
            Self::Signature => "The update's signature is not valid: nothing was installed.".into(),
            Self::Manifest => "Could not read the update's description.".into(),
            Self::NoArtifact => "This version has no download for this system.".into(),
            Self::Incomplete => "The download is incomplete: it resumes when retried.".into(),
            Self::Digest => "The download does not match what was signed: it was deleted.".into(),
            Self::Disk => "Could not write the update to disk.".into(),
            Self::NotInstalled => "This copy of quill was not installed with quill --install: update it from where you built it.".into(),
            Self::Replayed => "The update server offered an older release than one it offered before: nothing was installed.".into(),
            Self::Expired => "The update's description has expired: nothing was installed.".into(),
            Self::Foreign => "The update is not one of quill's stable releases: nothing was installed.".into(),
            Self::Interrupted => "The update was interrupted. Retry to finish it.".into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label())
    }
}

impl std::error::Error for Error {}

/// This platform's key in a manifest: `linux-x86_64`, `windows-x86_64`…
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// The keys releases are checked with: `stored`, the ones a rotation left, else the ones
/// compiled in; in a debug build, `QUILL_UPDATE_KEY` instead when set, for a test feed signed
/// with a throwaway key. A release build never reads it.
pub fn trusted_keys(stored: &[VerifyingKey]) -> Vec<VerifyingKey> {
    #[cfg(debug_assertions)]
    if let Ok(key) = std::env::var("QUILL_UPDATE_KEY") {
        return parse_key(&key).into_iter().collect();
    }
    if stored.is_empty() {
        compiled_keys()
    } else {
        stored.to_vec()
    }
}

/// The release key and the backup key compiled in, whatever the environment says.
pub fn compiled_keys() -> Vec<VerifyingKey> {
    [PUBLIC_KEY, BACKUP_KEY]
        .into_iter()
        .filter_map(parse_key)
        .collect()
}

/// `keys` as text, one base64 key each, as they are kept.
pub fn write_keys(keys: &[VerifyingKey]) -> String {
    let base64 = base64::engine::general_purpose::STANDARD;
    let keys: Vec<String> = keys
        .iter()
        .map(|key| base64.encode(key.to_bytes()))
        .collect();
    keys.join(" ")
}

/// Keys kept by [`write_keys`]; the text is refused whole when one is not a key.
pub fn read_keys(text: &str) -> Option<Vec<VerifyingKey>> {
    text.split_whitespace().map(parse_key).collect()
}

/// A public key written as base64; a weak (small-order) one is refused.
pub fn parse_key(text: &str) -> Option<VerifyingKey> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .ok()?;
    let key = VerifyingKey::from_bytes(&bytes.try_into().ok()?).ok()?;
    (!key.is_weak()).then_some(key)
}

/// The manifest in `manifest`, once `signature` (base64) proves the key's owner signed those
/// exact bytes.
pub fn verify(manifest: &[u8], signature: &str, key: &VerifyingKey) -> Result<Manifest, Error> {
    verify_signed(manifest, &[signature], std::slice::from_ref(key))
}

/// The manifest in `manifest`, once one of `signatures` (each base64) proves the owner of one
/// of `trusted` signed those exact bytes. When it names `next_keys`, every one of them not
/// trusted yet must have signed it too, or it is refused; a set that differs from `trusted` is
/// a rotation, left in [`Manifest::rotated`].
pub fn verify_signed(
    manifest: &[u8],
    signatures: &[&str],
    trusted: &[VerifyingKey],
) -> Result<Manifest, Error> {
    let signatures: Vec<Signature> = signatures
        .iter()
        .filter_map(|text| {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(text.trim())
                .ok()?;
            Signature::from_slice(&bytes).ok()
        })
        .collect();
    let signed_by = |key: &VerifyingKey| {
        (signatures.iter()).any(|signature| key.verify_strict(manifest, signature).is_ok())
    };
    if !trusted.iter().any(signed_by) {
        return Err(Error::Signature);
    }
    // Signed by the owner, so its fields are safe to name in the log.
    let mut read: Manifest = serde_json::from_slice(manifest).map_err(|error| {
        eprintln!("quill: update: unreadable manifest: {error}");
        Error::Manifest
    })?;
    if !read.next_keys.is_empty() {
        let next: Vec<VerifyingKey> = (read.next_keys.iter())
            .map(|text| parse_key(text))
            .collect::<Option<_>>()
            .ok_or(Error::Manifest)?;
        let new = next.iter().filter(|key| !trusted.contains(key));
        if !new.clone().all(signed_by) {
            eprintln!("quill: update: a rotation not signed by its new keys");
            return Err(Error::Signature);
        }
        if new.count() > 0 || trusted.iter().any(|key| !next.contains(key)) {
            read.rotated = Some(next);
        }
    }
    Ok(read)
}

/// What a copy expects of a manifest beyond its signature: the highest sequence it saw.
#[derive(Clone, Debug, Default)]
pub struct Trust {
    /// The keys to verify with ([`trusted_keys`]); none trusts nothing.
    pub keys: Vec<VerifyingKey>,
    pub sequence: u64,
}

impl Trust {
    pub fn new(keys: Vec<VerifyingKey>) -> Self {
        Self { keys, sequence: 0 }
    }
}

/// `manifest`, verified, is quill's stable channel, not lower than `sequence` and not expired
/// at `now` (seconds since 1970).
pub fn admit(manifest: &Manifest, sequence: u64, now: u64) -> Result<(), Error> {
    if manifest.product != PRODUCT || manifest.channel != CHANNEL {
        return Err(Error::Foreign);
    }
    if manifest.sequence < sequence {
        return Err(Error::Replayed);
    }
    if let Some(expires) = &manifest.expires {
        // One that cannot be read is as good as expired.
        if rfc3339(expires).is_none_or(|at| at <= now as i64) {
            return Err(Error::Expired);
        }
    }
    Ok(())
}

/// The seconds since 1970 of an RFC 3339 date and time (`2026-11-02T12:00:00Z`,
/// `…-03:00`, fractions of a second ignored).
pub fn rfc3339(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    let separators = [(4, b'-'), (7, b'-'), (13, b':'), (16, b':')];
    if bytes.len() < 20
        || !separators.iter().all(|(at, byte)| bytes[*at] == *byte)
        || !matches!(bytes[10], b'T' | b't')
    {
        return None;
    }
    let number = |from: usize, to: usize| -> Option<i64> {
        let digits = text.get(from..to)?;
        digits
            .bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| digits.parse().ok())?
    };
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    let mut rest = &text[19..];
    if let Some(fraction) = rest.strip_prefix('.') {
        rest = fraction.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let offset = match rest.as_bytes() {
        [b'Z' | b'z'] => 0,
        [sign @ (b'+' | b'-'), _, _, b':', _, _] => {
            let at = text.len() - 6;
            let minutes = number(at + 1, at + 3)? * 60 + number(at + 4, at + 6)?;
            if *sign == b'-' {
                -minutes
            } else {
                minutes
            }
        }
        _ => return None,
    };
    // Days since 1970 of a civil date (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let of_era = y - era * 400;
    let of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let days = era * 146_097 + of_era * 365 + of_era / 4 - of_era / 100 + of_year - 719_468;
    Some(days * 86_400 + hour * 3600 + minute * 60 + second.min(60) - offset * 60)
}

/// What `manifest` offers a program at `installed` on `platform`: nothing when it is not newer.
/// An artifact that could not be installed (too large, not HTTPS, no digest) counts as none.
pub fn offer(manifest: Manifest, installed: &Version, platform: &str) -> Option<Release> {
    if manifest.version <= *installed {
        return None;
    }
    let mandatory = manifest
        .minimum_version
        .as_ref()
        .is_some_and(|minimum| installed < minimum);
    let artifact = manifest
        .platforms
        .get(platform)
        .filter(|artifact| usable(artifact))
        .cloned();
    let whats_new =
        (manifest.whats_new).filter(|file| usable(file) && file.size <= whats_new::MAX_JSON);
    let files = (manifest.files.get(platform))
        .filter(|file| usable(file) && file.size <= files::MAX_INDEX)
        .cloned();
    Some(Release {
        version: manifest.version,
        date: manifest.date,
        notes: manifest.notes,
        mandatory,
        artifact,
        whats_new,
        files,
    })
}

/// An address quill may download from: HTTPS (or a test feed on this machine), a size within
/// bounds and a digest.
pub(crate) fn usable(artifact: &Artifact) -> bool {
    let https = artifact.url.starts_with("https://")
        // The test feeds of `serve`, on this machine only.
        || artifact.url.starts_with("http://127.0.0.1:");
    https
        && artifact.size > 0
        && artifact.size <= MAX_ARTIFACT
        && artifact.sha256.len() == 64
        && artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Whether an annotated tag's `message` makes its version mandatory: it has the word
/// «mandatory», in any case.
pub fn mandatory(message: &str) -> bool {
    message
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| word.to_lowercase() == MANDATORY_WORD)
}

/// The newest mandatory version up to `version` among `tags`, each an annotated tag's name
/// (`v1.2.0`) and message. Names that are not a `v` and a version are ignored.
pub fn minimum_version<'a>(
    version: &Version,
    tags: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Option<Version> {
    tags.into_iter()
        .filter(|(_, message)| mandatory(message))
        .filter_map(|(name, _)| Version::parse(name.strip_prefix('v')?).ok())
        .filter(|tagged| tagged <= version)
        .max()
}

/// Where a release's files are read from: the files of one release at a base address.
#[derive(Clone, Debug)]
pub struct Source {
    pub base: String,
}

impl Source {
    /// Where this copy looks: `QUILL_UPDATE_FEED` when set (a test feed), else the latest
    /// release of [`REPOSITORY`], at its public addresses, with no token.
    pub fn locate() -> Self {
        if let Some(base) = std::env::var("QUILL_UPDATE_FEED")
            .ok()
            .filter(|base| !base.trim().is_empty())
        {
            return Self {
                base: base.trim_end_matches('/').to_owned(),
            };
        }
        Self {
            base: format!("https://github.com/{REPOSITORY}/releases/latest/download"),
        }
    }
}

/// The agent every request goes through: bounded waits, its own name, and no redirects
/// followed by itself (see [`request`]).
pub fn agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .timeout_per_read(Some(Duration::from_secs(30)))
            .user_agent(concat!("quill-updater/", env!("CARGO_PKG_VERSION")))
            .build(),
    )
}

/// The signed manifest of `source`'s latest release, its artifacts' addresses being the ones
/// to download from, once [`verify_signed`] with `trust`'s keys and [`admit`] let it through.
pub fn check(agent: &ureq::Agent, source: &Source, trust: &Trust) -> Result<Manifest, Error> {
    if trust.keys.is_empty() {
        return Err(Error::NoKey);
    }
    let base = &source.base;
    let manifest = fetch(agent, &format!("{base}/{MANIFEST}"), MAX_MANIFEST)?;
    let signature = fetch(agent, &format!("{base}/{SIGNATURE}"), MAX_SIGNATURE)?;
    let more = match fetch(agent, &format!("{base}/{SIGNATURES}"), MAX_SIGNATURES) {
        Err(Error::Status(404)) => Vec::new(),
        more => more?,
    };
    let manifest = verify_signed(&manifest, &signatures(&signature, &more)?, &trust.keys)?;
    admit(&manifest, trust.sequence, now())?;
    Ok(manifest)
}

/// The signature and the lines of the further ones.
fn signatures<'a>(signature: &'a [u8], more: &'a [u8]) -> Result<Vec<&'a str>, Error> {
    let mut all = vec![text(signature)?];
    all.extend(text(more)?.lines().filter(|line| !line.trim().is_empty()));
    Ok(all)
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn text(bytes: &[u8]) -> Result<&str, Error> {
    std::str::from_utf8(bytes).map_err(|_| Error::Signature)
}

/// A GET of `url`, from byte `offset` on when given. Redirects are followed here, five at
/// most: GitHub sends a release's file on to its storage host.
fn request(
    agent: &ureq::Agent,
    url: &str,
    offset: Option<u64>,
) -> Result<ureq::http::Response<ureq::Body>, Error> {
    let mut url = url.to_owned();
    for _ in 0..5 {
        let mut request = agent.get(&url);
        if let Some(offset) = offset {
            request = request.header("Range", format!("bytes={offset}-"));
        }
        let response = request.call().map_err(network)?;
        if !response.status().is_redirection() {
            return Ok(response);
        }
        url = response
            .headers()
            .get("location")
            .and_then(|location| location.to_str().ok())
            .ok_or(Error::Network)?
            .to_owned();
    }
    Err(Error::Network)
}

/// The body of `url` when it answers 2xx, read up to `limit` bytes.
pub(crate) fn fetch(agent: &ureq::Agent, url: &str, limit: usize) -> Result<Vec<u8>, Error> {
    let response = request(agent, url, None)?;
    let status = response.status().as_u16();
    if !response.status().is_success() {
        return Err(Error::Status(status));
    }
    read_limited(&mut response.into_body().into_reader(), limit)
}

/// Everything `reader` gives, unless it is more than `limit` bytes.
fn read_limited(reader: &mut impl Read, limit: usize) -> Result<Vec<u8>, Error> {
    let mut body = Vec::new();
    let mut chunk = [0; 16 * 1024];
    loop {
        let read = reader.read(&mut chunk).map_err(network)?;
        if read == 0 {
            return Ok(body);
        }
        if body.len() + read > limit {
            return Err(Error::TooLarge);
        }
        body.extend_from_slice(&chunk[..read]);
    }
}

fn network(error: impl fmt::Display) -> Error {
    eprintln!("quill: update: {error}");
    Error::Network
}

pub(crate) fn disk(error: io::Error) -> Error {
    eprintln!("quill: update: {error}");
    Error::Disk
}

/// Download `artifact` into `folder`, telling `progress` the bytes there and the whole; when it
/// answers false the download stops there ([`Error::Interrupted`]), its part kept. A partial
/// file left by an earlier try (named by the digest, so another release never resumes it)
/// continues where it ended. Returns the file once its size and SHA-256 match; one that does
/// not match is deleted.
pub fn download(
    agent: &ureq::Agent,
    artifact: &Artifact,
    folder: &Path,
    mut progress: impl FnMut(u64, u64) -> bool,
) -> Result<PathBuf, Error> {
    let digest = artifact.sha256.to_ascii_lowercase();
    let done = folder.join(format!("quill-{digest}"));
    let part = folder.join(format!("quill-{digest}.part"));
    std::fs::create_dir_all(folder).map_err(disk)?;
    // Another release's download, done or partial, is never resumed: it goes.
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("quill-") && !name.starts_with(&format!("quill-{digest}")) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    // Downloaded by an earlier try whose installation failed.
    if sha256(&done).is_ok_and(|found| found == digest) {
        return Ok(done);
    }
    fetch_to(agent, &artifact.url, artifact.size, &part, &mut progress)?;
    if sha256(&part).map_err(disk)? != digest {
        let _ = std::fs::remove_file(&part);
        return Err(Error::Digest);
    }
    std::fs::rename(&part, &done).map_err(disk)?;
    Ok(done)
}

/// Download `url` into `part` until it holds `size` bytes, continuing what it holds; more than
/// `size` deletes it ([`Error::Digest`]). `progress` hears the bytes there and the whole, and
/// stops it by answering false ([`Error::Interrupted`]), its part kept.
pub(crate) fn fetch_to(
    agent: &ureq::Agent,
    url: &str,
    size: u64,
    part: &Path,
    progress: &mut impl FnMut(u64, u64) -> bool,
) -> Result<(), Error> {
    let mut have = std::fs::metadata(part).map_or(0, |found| found.len());
    if have > size {
        have = 0;
    }
    if have < size {
        let offset = (have > 0).then_some(have);
        let response = request(agent, url, offset)?;
        let status = response.status();
        let resumed = status == ureq::http::StatusCode::PARTIAL_CONTENT
            && have > 0
            && response
                .headers()
                .get("content-range")
                .and_then(|range| range.to_str().ok())
                .is_some_and(|range| range.starts_with(&format!("bytes {have}-")));
        if !resumed {
            if !status.is_success() {
                return Err(Error::Status(status.as_u16()));
            }
            // The server sends it whole: start over.
            have = 0;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(resumed)
            .truncate(!resumed)
            .open(part)
            .map_err(disk)?;
        let mut body = response.into_body().into_reader();
        let mut chunk = vec![0; 64 * 1024];
        let streamed = loop {
            if !progress(have, size) {
                break Err(Error::Interrupted);
            }
            match body.read(&mut chunk) {
                Ok(0) => break Ok(()),
                Ok(read) => {
                    have += read as u64;
                    if have > size {
                        drop(file);
                        let _ = std::fs::remove_file(part);
                        return Err(Error::Digest);
                    }
                    file.write_all(&chunk[..read]).map_err(disk)?;
                }
                Err(error) => break Err(network(error)),
            }
        };
        // What came is on the disk before an error returns, so the next try resumes from its
        // true end.
        file.sync_all().map_err(disk)?;
        streamed?;
    }
    if have != size {
        return Err(Error::Incomplete);
    }
    Ok(())
}

/// The SHA-256 of the file at `path`, lowercase hex.
pub(crate) fn sha256(path: &Path) -> io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

/// `bytes` as lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests;
