//! quill's own updates (`docs/UPDATES.md`).
//!
//! Every release publishes `quill-update.json`, a manifest of its version, notes, the oldest
//! version still accepted (`minimum_version`) and one artifact per platform (address, size,
//! SHA-256), and `quill-update.json.sig`, the Ed25519 signature of those exact bytes in base64.
//! The public half of the release key is compiled in (`release-key.pub`).
//!
//! The repository is private, so a check reads its latest release through the GitHub API with
//! a token ([`token`]), and downloads the artifacts the same way. A feed at a base address
//! (`QUILL_UPDATE_FEED`, for the tests) needs no token. The signature is verified before the
//! JSON is read, and an artifact is installed only once its size and digest match what was
//! signed ([`install`]). Network, disk and every wait run on the caller's thread: never the
//! interface's.

pub mod install;
#[cfg(any(test, feature = "serve"))]
pub mod serve;
pub mod whats_new;

use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use base64::Engine as _;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use semver::Version;

/// This program's version: the workspace's, which a release's tag matches.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The repository the releases are published in.
pub const REPOSITORY: &str = "Niiihuel/quill";
/// Whether the repository is private: its files are then read through the GitHub API with a
/// token. A public repository is read at its releases' addresses: set this to false then.
pub const PRIVATE: bool = true;
const API: &str = "https://api.github.com";
pub const MANIFEST: &str = "quill-update.json";
pub const SIGNATURE: &str = "quill-update.json.sig";
/// The word in an annotated tag's message that makes its version mandatory.
pub const MANDATORY_WORD: &str = "mandatory";

const MAX_MANIFEST: usize = 64 * 1024;
const MAX_SIGNATURE: usize = 1024;
const MAX_RELEASE: usize = 1024 * 1024;
/// No artifact is larger: a manifest that says so is refused.
pub const MAX_ARTIFACT: u64 = 1 << 30;

/// The release key's public half, base64, written by `quill-release keygen`. Empty until the
/// owner made one: then nothing is trusted, and no update is ever installed.
const PUBLIC_KEY: &str = include_str!("../release-key.pub");

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
    /// The release's files by name, with their addresses in the private repository's API, which
    /// the media of `whats_new` are fetched from. Never signed: the check fills it.
    #[serde(skip)]
    pub assets: BTreeMap<String, String>,
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
    /// See [`Manifest::assets`].
    pub assets: BTreeMap<String, String>,
}

/// Why a check, a download or an installation did not go through. Each leaves quill as it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// This build has no release key: nothing can be trusted.
    NoKey,
    /// The private repository refused the token, or there is none.
    Access,
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
    /// The job ended without saying how (its thread stopped early).
    Interrupted,
}

impl Error {
    /// What the person reads.
    pub fn label(self) -> String {
        match self {
            Self::NoKey => "This build has no update key: no update can be trusted.".into(),
            Self::Access => "Updates need access to the private repository (set QUILL_UPDATE_TOKEN or sign in with gh).".into(),
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

/// The key releases are checked with: the compiled one; in a debug build, `QUILL_UPDATE_KEY`
/// instead when set, for a test feed signed with a throwaway key. A release build never reads
/// it.
pub fn public_key() -> Option<VerifyingKey> {
    #[cfg(debug_assertions)]
    if let Ok(key) = std::env::var("QUILL_UPDATE_KEY") {
        return parse_key(&key);
    }
    release_key()
}

/// The release key compiled in, whatever the environment says.
pub fn release_key() -> Option<VerifyingKey> {
    parse_key(PUBLIC_KEY)
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
    let signature = base64::engine::general_purpose::STANDARD
        .decode(signature.trim())
        .ok()
        .and_then(|bytes| Signature::from_slice(&bytes).ok())
        .ok_or(Error::Signature)?;
    key.verify_strict(manifest, &signature)
        .map_err(|_| Error::Signature)?;
    // Signed by the owner, so its fields are safe to name in the log.
    serde_json::from_slice(manifest).map_err(|error| {
        eprintln!("quill: update: unreadable manifest: {error}");
        Error::Manifest
    })
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
    Some(Release {
        version: manifest.version,
        date: manifest.date,
        notes: manifest.notes,
        mandatory,
        artifact,
        whats_new,
        assets: manifest.assets,
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

/// Where a release's files are read from. Not `Debug`: it holds the token.
#[derive(Clone)]
pub enum Source {
    /// Files at a base address: the test feed (`QUILL_UPDATE_FEED`), or a public repository's
    /// releases.
    Files { base: String },
    /// A private repository's latest release, through its API with a token. `api` is the API's
    /// base address (`https://api.github.com`, or a test server's).
    Api { token: String, api: String },
}

impl Source {
    /// Where this copy looks: `QUILL_UPDATE_FEED` when set, else the repository's. `None` when
    /// the repository is private and no token is found.
    pub fn locate() -> Option<Self> {
        if let Some(base) = std::env::var("QUILL_UPDATE_FEED")
            .ok()
            .filter(|base| !base.trim().is_empty())
        {
            return Some(Self::Files {
                base: base.trim_end_matches('/').to_owned(),
            });
        }
        if !PRIVATE {
            return Some(Self::Files {
                base: format!("https://github.com/{REPOSITORY}/releases/latest/download"),
            });
        }
        Some(Self::Api {
            token: token()?,
            api: API.to_owned(),
        })
    }

    /// The token and the API base a request to the API carries, if any.
    pub(crate) fn grant(&self) -> Option<(&str, &str)> {
        match self {
            Self::Api { token, api } => Some((token, api)),
            Self::Files { .. } => None,
        }
    }
}

/// The token that reads the private repository: `QUILL_UPDATE_TOKEN`, else the output of
/// `gh auth token`, waited for five seconds at most. It is never logged.
pub fn token() -> Option<String> {
    if let Some(token) = std::env::var("QUILL_UPDATE_TOKEN")
        .ok()
        .map(|token| token.trim().to_owned())
        .filter(|token| !token.is_empty())
    {
        return Some(token);
    }
    let mut child = quill_files::command("gh")
        .args(["auth", "token"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait().ok()? {
            Some(status) => {
                let mut out = String::new();
                child.stdout.take()?.read_to_string(&mut out).ok()?;
                let token = out.trim().to_owned();
                return (status.success() && !token.is_empty()).then_some(token);
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(50)),
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
/// to download from.
pub fn check(
    agent: &ureq::Agent,
    source: &Source,
    key: Option<&VerifyingKey>,
) -> Result<Manifest, Error> {
    let key = key.ok_or(Error::NoKey)?;
    match source {
        Source::Files { base } => {
            let manifest = fetch(agent, &format!("{base}/{MANIFEST}"), None, MAX_MANIFEST)?;
            let signature = fetch(agent, &format!("{base}/{SIGNATURE}"), None, MAX_SIGNATURE)?;
            verify(&manifest, text(&signature)?, key)
        }
        Source::Api { token, api } => {
            let assets = latest(agent, token, api)?;
            let asset = |name: &str| assets.get(name).ok_or(Error::Manifest);
            let manifest = fetch(agent, asset(MANIFEST)?, Some((token, api)), MAX_MANIFEST)?;
            let signature = fetch(agent, asset(SIGNATURE)?, Some((token, api)), MAX_SIGNATURE)?;
            let mut manifest = verify(&manifest, text(&signature)?, key)?;
            // The signed addresses name the release's files; the API reaches the same files.
            for artifact in manifest.platforms.values_mut() {
                artifact.url = via(&assets, &artifact.url);
            }
            if let Some(file) = manifest.whats_new.as_mut() {
                file.url = via(&assets, &file.url);
            }
            manifest.assets = assets;
            Ok(manifest)
        }
    }
}

/// Where the API serves the release file `url` names (by its last part), else `url` itself.
pub(crate) fn via(assets: &BTreeMap<String, String>, url: &str) -> String {
    let name = url.rsplit('/').next().unwrap_or_default();
    assets.get(name).cloned().unwrap_or_else(|| url.to_owned())
}

fn text(bytes: &[u8]) -> Result<&str, Error> {
    std::str::from_utf8(bytes).map_err(|_| Error::Signature)
}

#[derive(Deserialize)]
struct Latest {
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    /// The asset's address in the API, which a download of it follows.
    url: String,
}

/// The latest release's assets by name, each with its address in the API.
fn latest(agent: &ureq::Agent, token: &str, api: &str) -> Result<BTreeMap<String, String>, Error> {
    let response = agent
        .get(&format!("{api}/repos/{REPOSITORY}/releases/latest"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .map_err(network)?;
    let status = response.status().as_u16();
    if !response.status().is_success() {
        return Err(status_error(status, true));
    }
    let body = read_limited(&mut response.into_body().into_reader(), MAX_RELEASE)?;
    let latest: Latest = serde_json::from_slice(&body).map_err(|_| Error::Manifest)?;
    Ok(latest
        .assets
        .into_iter()
        .map(|asset| (asset.name, asset.url))
        .collect())
}

/// A GET of `url`, from byte `offset` on when given. The token goes to the API only, and
/// redirects are followed here, without it: the API sends a download on to a storage host that
/// must not see it.
fn request(
    agent: &ureq::Agent,
    url: &str,
    grant: Option<(&str, &str)>,
    offset: Option<u64>,
) -> Result<ureq::http::Response<ureq::Body>, Error> {
    let mut url = url.to_owned();
    let mut grant = grant;
    for _ in 0..5 {
        let mut request = agent.get(&url);
        if let Some((token, api)) = grant {
            if url.starts_with(api) {
                request = request
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Accept", "application/octet-stream");
            }
        }
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
        grant = None;
    }
    Err(Error::Network)
}

/// The body of `url` when it answers 2xx, read up to `limit` bytes.
pub(crate) fn fetch(
    agent: &ureq::Agent,
    url: &str,
    grant: Option<(&str, &str)>,
    limit: usize,
) -> Result<Vec<u8>, Error> {
    let response = request(agent, url, grant, None)?;
    let status = response.status().as_u16();
    if !response.status().is_success() {
        return Err(status_error(status, grant.is_some()));
    }
    read_limited(&mut response.into_body().into_reader(), limit)
}

/// What a refused answer means: with a token, a 401, 403 or 404 is about access.
fn status_error(status: u16, with_token: bool) -> Error {
    match status {
        401 | 403 | 404 if with_token => Error::Access,
        _ => Error::Status(status),
    }
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

/// Download `artifact` into `folder`, telling `progress` the bytes there and the whole. A
/// partial file left by an earlier try (named by the digest, so another release never resumes
/// it) continues where it ended. Returns the file once its size and SHA-256 match; one that
/// does not match is deleted.
pub fn download(
    agent: &ureq::Agent,
    source: &Source,
    artifact: &Artifact,
    folder: &Path,
    mut progress: impl FnMut(u64, u64),
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
    let mut have = std::fs::metadata(&part).map_or(0, |found| found.len());
    if have > artifact.size {
        have = 0;
    }
    if have < artifact.size {
        let offset = (have > 0).then_some(have);
        let response = request(agent, &artifact.url, source.grant(), offset)?;
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
                return Err(status_error(status.as_u16(), source.grant().is_some()));
            }
            // The server sends it whole: start over.
            have = 0;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(resumed)
            .truncate(!resumed)
            .open(&part)
            .map_err(disk)?;
        progress(have, artifact.size);
        let mut body = response.into_body().into_reader();
        let mut chunk = vec![0; 64 * 1024];
        let streamed = loop {
            match body.read(&mut chunk) {
                Ok(0) => break Ok(()),
                Ok(read) => {
                    have += read as u64;
                    if have > artifact.size {
                        drop(file);
                        let _ = std::fs::remove_file(&part);
                        return Err(Error::Digest);
                    }
                    file.write_all(&chunk[..read]).map_err(disk)?;
                    progress(have, artifact.size);
                }
                Err(error) => break Err(network(error)),
            }
        };
        // What came is on the disk before an error returns, so the next try resumes from its
        // true end.
        file.sync_all().map_err(disk)?;
        streamed?;
    }
    if have != artifact.size {
        return Err(Error::Incomplete);
    }
    if sha256(&part).map_err(disk)? != digest {
        let _ = std::fs::remove_file(&part);
        return Err(Error::Digest);
    }
    std::fs::rename(&part, &done).map_err(disk)?;
    Ok(done)
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
