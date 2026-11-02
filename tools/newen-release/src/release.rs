//! The release side of quill's updates, for the owner's machine and the
//! release workflow (`docs/UPDATES.md`):
//!
//! ```text
//! quill-release keygen <private-key-file>   a new key: the private half to the file, the public to stdout
//! quill-release tag <tag>                   the tag is annotated and names this workspace's version
//! quill-release whats-new <tag> <base-url> <out-dir>
//!                                          whats-new.json and its media from release/notes/<version>.toml
//!                                          and release/media/<version>/, into <out-dir>; prints each file
//! quill-release files <tag> <base-url> <out-dir> [--previous <dir>] <platform>=<folder>...
//!                                          files-<platform>.json of each unpacked bundle, and the
//!                                          blobs no earlier release published, into <out-dir>;
//!                                          prints each file written
//! quill-release manifest <tag> <base-url> [--whats-new <file>] [--previous <dir>]
//!                        [--files <platform>=<file>]... [--expires <rfc3339>] <platform>=<file>...
//!                                          the manifest of the release, to stdout; its sequence is
//!                                          the previous release's plus one
//! quill-release sign <manifest>             <manifest>.sig, with QUILL_UPDATE_SIGNING_KEY
//! quill-release cosign <manifest> <key-file>
//!                                          one more signature, a line of <manifest>.sigs (a rotation)
//! quill-release verify <manifest>           its signatures verify with the keys compiled in, and the
//!                                          whats-new.json, media, lists and new blobs beside it match
//!                                          what it names
//! ```
//!
//! `--previous` is a folder holding the latest published release's `quill-update.json`, its
//! signatures and its `files-*.json`: verified with the keys compiled in, it gives the sequence
//! and the blobs this release need not upload again.
//!
//! A private key is 32 bytes in base64 and never printed.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use base64::Engine as _;
use ed25519_dalek::{Signer, SigningKey};
use newen_update::files::{self, Bundle, Known};
use newen_update::whats_new::{self, Announcement, Kind, Medium, Notes, WhatsNew};
use newen_update::{Artifact, Manifest, VerifyingKey, Version};
use serde::Deserialize;
use sha2::{Digest, Sha256};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let result = match arguments.as_slice() {
        ["keygen", file] => keygen(Path::new(file)),
        ["tag", tag] => tag_version(tag).map(|version| println!("{version}")),
        ["whats-new", tag, base, out] => build_whats_new_for(tag, base, Path::new(out)),
        ["files", tag, base, out, rest @ ..] => options(rest)
            .and_then(|options| files_for(tag, base, Path::new(out), &options)),
        ["manifest", tag, base, rest @ ..] => {
            options(rest).and_then(|options| manifest(tag, base, &options))
        }
        ["sign", file] => sign(Path::new(file)),
        ["cosign", file, key] => cosign(Path::new(file), Path::new(key)),
        ["verify", file] => verify(Path::new(file)),
        _ => Err("usage: quill-release keygen <file> | tag <tag> | whats-new <tag> <base-url> <out-dir> | files <tag> <base-url> <out-dir> [--previous <dir>] <platform>=<folder>... | manifest <tag> <base-url> [--whats-new <file>] [--previous <dir>] [--files <platform>=<file>]... [--expires <rfc3339>] <platform>=<file>... | sign <manifest> | cosign <manifest> <key-file> | verify <manifest>".into()),
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
    if version.to_string() != newen_update::VERSION {
        return Err(format!(
            "{tag} does not match the workspace's version {}",
            newen_update::VERSION
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
        sha256: newen_update::hex(&Sha256::digest(&bytes)),
    })
}

fn file_name(file: &Path) -> Result<&str> {
    file.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("{}: no file name", file.display()))
}

/// A command's `--name value` options and its other arguments.
#[derive(Default)]
struct Options {
    whats_new: Option<PathBuf>,
    previous: Option<PathBuf>,
    expires: Option<String>,
    /// `--files`, each `<platform>=<file>`.
    files: Vec<String>,
    /// Each `<platform>=<file or folder>`.
    rest: Vec<String>,
}

fn options(arguments: &[&str]) -> Result<Options> {
    let mut options = Options::default();
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        let mut value = || {
            arguments
                .next()
                .map(|value| value.to_string())
                .ok_or_else(|| format!("{argument} needs a value"))
        };
        match *argument {
            "--whats-new" => options.whats_new = Some(value()?.into()),
            "--previous" => options.previous = Some(value()?.into()),
            "--expires" => options.expires = Some(value()?),
            "--files" => options.files.push(value()?),
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            other => options.rest.push(other.to_owned()),
        }
    }
    if options.rest.is_empty() {
        return Err("no <platform>=<file> given".into());
    }
    Ok(options)
}

/// `<platform>=<path>`.
fn pair(text: &str) -> Result<(&str, &Path)> {
    let (platform, path) = text
        .split_once('=')
        .ok_or_else(|| format!("{text}: <platform>=<path>"))?;
    Ok((platform, Path::new(path)))
}

/// The release published before, from the folder `dir` (`--previous`): its manifest, verified
/// with `keys`, and the lists of files it names, each matching its digest.
fn read_previous(dir: &Path, keys: &[VerifyingKey]) -> Result<(Manifest, Vec<Bundle>)> {
    let manifest = read_signed(&dir.join(newen_update::MANIFEST), keys)?;
    let mut bundles = Vec::new();
    for (platform, entry) in &manifest.files {
        let bytes = matching(dir, entry)?;
        bundles.push(
            serde_json::from_slice(&bytes)
                .map_err(|error| format!("{platform}'s list of files: {error}"))?,
        );
    }
    Ok((manifest, bundles))
}

/// The manifest at `file`, once `<file>.sig` or a line of `<file>.sigs` verifies with one of
/// `keys` (and a rotation it names is signed by its new keys).
fn read_signed(file: &Path, keys: &[VerifyingKey]) -> Result<Manifest> {
    let bytes = std::fs::read(file).map_err(|error| format!("{}: {error}", file.display()))?;
    let signature = std::fs::read_to_string(signature_path(file))
        .map_err(|error| format!("{}.sig: {error}", file.display()))?;
    let more = std::fs::read_to_string(more_signatures_path(file)).unwrap_or_default();
    let signatures: Vec<&str> = std::iter::once(signature.as_str())
        .chain(more.lines().filter(|line| !line.trim().is_empty()))
        .collect();
    newen_update::verify_signed(&bytes, &signatures, keys).map_err(|error| {
        format!("{}: {error:?}: it does not verify with the keys compiled in (another signing secret, or a changed manifest)", file.display())
    })
}

/// The sequence of a release of `version` after `previous`: one more, the same for the same
/// version (a release made again), and never a version older than the one published.
fn sequence(version: &Version, previous: Option<&Manifest>) -> Result<u64> {
    match previous {
        None => Ok(1),
        Some(previous) if previous.version == *version => Ok(previous.sequence.max(1)),
        Some(previous) if previous.version > *version => Err(format!(
            "{} is published already, newer than {version}",
            previous.version
        )),
        Some(previous) => Ok(previous.sequence + 1),
    }
}

fn manifest(tag: &str, base: &str, options: &Options) -> Result<()> {
    let version = tag_version(tag)?;
    let tags = annotated_tags()?;
    let tag_notes = tags
        .iter()
        .find(|(name, _)| name == tag)
        .map(|(_, message)| message.clone())
        .unwrap_or_default();
    let minimum_version = newen_update::minimum_version(
        &version,
        tags.iter()
            .map(|(name, message)| (name.as_str(), message.as_str())),
    );
    let date = tag_date(tag)?;
    let previous = match &options.previous {
        Some(dir) => Some(read_previous(dir, &newen_update::compiled_keys())?.0),
        None => None,
    };
    let mut platforms = std::collections::BTreeMap::new();
    for artifact in &options.rest {
        let (platform, file) = pair(artifact)?;
        platforms.insert(platform.to_owned(), served(base, file, file_name(file)?)?);
    }
    let mut lists = std::collections::BTreeMap::new();
    for list in &options.files {
        let (platform, file) = pair(list)?;
        lists.insert(platform.to_owned(), served(base, file, file_name(file)?)?);
    }
    let whats_new = match &options.whats_new {
        Some(file) => Some(served(base, file, whats_new::FILE)?),
        None => None,
    };
    if let Some(expires) = &options.expires {
        newen_update::rfc3339(expires).ok_or_else(|| format!("{expires}: not RFC 3339"))?;
    }
    let manifest = Manifest {
        sequence: sequence(&version, previous.as_ref())?,
        version,
        date,
        notes: tag_notes,
        minimum_version,
        platforms,
        whats_new,
        expires: options.expires.clone(),
        files: lists,
        ..Default::default()
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&manifest).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn files_for(tag: &str, base: &str, out: &Path, options: &Options) -> Result<()> {
    let version = tag_version(tag)?;
    let previous = match &options.previous {
        Some(dir) => read_previous(dir, &newen_update::compiled_keys())?.1,
        None => Vec::new(),
    };
    let trees: Vec<(&str, &Path)> = options
        .rest
        .iter()
        .map(|tree| pair(tree))
        .collect::<Result<_>>()?;
    for file in build_files(&version, base, out, &previous, &trees)? {
        println!("{}", file.display());
    }
    Ok(())
}

/// Each bundle of `trees` (platform, unpacked folder) listed into `out` as
/// `files-<platform>.json`, with the blobs neither `previous` (earlier releases' lists) nor a
/// platform before it published. The files written: what the release uploads.
fn build_files(
    version: &Version,
    base: &str,
    out: &Path,
    previous: &[Bundle],
    trees: &[(&str, &Path)],
) -> Result<Vec<PathBuf>> {
    let mut known: Known = files::known(previous);
    let mut written = Vec::new();
    for (platform, tree) in trees {
        let (bundle, blobs) = files::pack(tree, version, platform, base, &known, out)
            .map_err(|error| format!("{}: {error}", tree.display()))?;
        if !files::valid(&bundle) {
            return Err(format!("{platform}: a path or link quill would refuse"));
        }
        let new: u64 = (bundle.files.iter())
            .filter(|file| {
                blobs
                    .iter()
                    .any(|blob| blob.ends_with(files::blob_name(&file.sha256)))
            })
            .map(|file| file.packed)
            .sum();
        eprintln!(
            "{platform}: {} files, {} new blobs ({new} bytes), the rest published before",
            bundle.files.len(),
            blobs.len()
        );
        known.extend(files::known(std::slice::from_ref(&bundle)));
        let list = out.join(files::index_name(platform));
        let json = serde_json::to_vec(&bundle).map_err(|error| error.to_string())?;
        std::fs::write(&list, json).map_err(|error| format!("{}: {error}", list.display()))?;
        written.push(list);
        written.extend(blobs);
    }
    Ok(written)
}

/// The bytes of the file `entry` names, in `folder` under its served name, when they are what
/// it says.
fn matching(folder: &Path, entry: &Artifact) -> Result<Vec<u8>> {
    let name = entry.url.rsplit('/').next().unwrap_or_default();
    let path = folder.join(name);
    let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    if bytes.len() as u64 != entry.size
        || newen_update::hex(&Sha256::digest(&bytes)) != entry.sha256
    {
        return Err(format!("{name} is not the file the release names"));
    }
    Ok(bytes)
}

fn signature_path(file: &Path) -> std::path::PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".sig");
    name.into()
}

fn more_signatures_path(file: &Path) -> std::path::PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".sigs");
    name.into()
}

/// A signing key in base64, as `keygen` writes it.
fn signing_key(text: &str) -> Option<SigningKey> {
    let seed: [u8; 32] = base64().decode(text.trim()).ok()?.try_into().ok()?;
    Some(SigningKey::from_bytes(&seed))
}

/// The signature of the manifest at `file`, base64, once it reads as one quill accepts.
fn signature_of(file: &Path, key: &SigningKey) -> Result<String> {
    let bytes = std::fs::read(file).map_err(|error| format!("{}: {error}", file.display()))?;
    serde_json::from_slice::<Manifest>(&bytes)
        .map_err(|error| format!("not a manifest: {error}"))?;
    Ok(base64().encode(key.sign(&bytes).to_bytes()))
}

/// One more signature of the manifest at `file`, with the key in `key_file`, as a line of
/// `<file>.sigs`: a rotation's new key, or the old one for copies that have not rotated.
fn cosign(file: &Path, key_file: &Path) -> Result<()> {
    let text = std::fs::read_to_string(key_file)
        .map_err(|error| format!("{}: {error}", key_file.display()))?;
    let key =
        signing_key(&text).ok_or("the key file does not hold a key from `quill-release keygen`")?;
    let signature = signature_of(file, &key)?;
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(more_signatures_path(file))
        .map_err(|error| error.to_string())?;
    writeln!(out, "{signature}").map_err(|error| error.to_string())
}

fn sign(file: &Path) -> Result<()> {
    let seed = std::env::var("QUILL_UPDATE_SIGNING_KEY")
        .map_err(|_| "QUILL_UPDATE_SIGNING_KEY is not set")?;
    let key = signing_key(&seed)
        .ok_or("QUILL_UPDATE_SIGNING_KEY is not a key from `quill-release keygen`")?;
    // Only what quill would accept is signed.
    let signature = signature_of(file, &key)?;
    std::fs::write(signature_path(file), format!("{signature}\n"))
        .map_err(|error| error.to_string())
}

fn verify(file: &Path) -> Result<()> {
    let keys = newen_update::compiled_keys();
    if keys.is_empty() {
        return Err("no release key compiled in: run quill-release keygen and commit crates/quill-update/release-key.pub".into());
    }
    let manifest = read_signed(file, &keys)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    newen_update::admit(&manifest, 0, now).map_err(|error| format!("{error:?}: {error}"))?;
    let folder = file.parent().unwrap_or(Path::new("."));
    if let Some(media) = check_whats_new(&manifest, folder)? {
        println!("{} verified ({media} media)", whats_new::FILE);
    }
    for (platform, entry) in &manifest.files {
        let (new, reused) = check_files(&manifest, platform, entry, folder)?;
        println!("{platform}: list verified, {new} new blobs here and {reused} files from earlier releases");
    }
    if let Some(keys) = &manifest.rotated {
        println!("rotates to {} keys", keys.len());
    }
    println!(
        "{} verified (sequence {}, minimum {})",
        manifest.version,
        manifest.sequence,
        manifest
            .minimum_version
            .map_or("none".into(), |minimum| minimum.to_string())
    );
    Ok(())
}

/// The list `entry` names for `platform`, beside the manifest in `folder`, is what was signed
/// and quill would accept it, and each blob it names at this release's address is in `folder`
/// and unpacks to its file. How many such blobs, and how many files come from earlier
/// releases.
fn check_files(
    manifest: &Manifest,
    platform: &str,
    entry: &Artifact,
    folder: &Path,
) -> Result<(usize, usize)> {
    let bytes = matching(folder, entry)?;
    let bundle: Bundle = serde_json::from_slice(&bytes)
        .map_err(|error| format!("{platform}'s list of files: {error}"))?;
    if bundle.version != manifest.version || bundle.platform != platform || !files::valid(&bundle) {
        return Err(format!(
            "{platform}'s list of files is not one quill accepts"
        ));
    }
    let base = entry.url.rsplit_once('/').map_or("", |(base, _)| base);
    let (mut new, mut reused) = (std::collections::BTreeSet::new(), 0);
    for file in &bundle.files {
        if file.url.rsplit_once('/').map(|(at, _)| at) != Some(base) {
            reused += 1;
            continue;
        }
        if !new.insert(file.sha256.as_str()) {
            continue;
        }
        let blob = folder.join(files::blob_name(&file.sha256));
        let packed =
            std::fs::read(&blob).map_err(|error| format!("{}: {error}", blob.display()))?;
        let mut unpacked = Vec::new();
        std::io::Read::read_to_end(
            &mut flate2::read::GzDecoder::new(packed.as_slice()),
            &mut unpacked,
        )
        .map_err(|error| format!("{}: {error}", blob.display()))?;
        if packed.len() as u64 != file.packed
            || unpacked.len() as u64 != file.size
            || newen_update::hex(&Sha256::digest(&unpacked)) != file.sha256
        {
            return Err(format!("{}: not the blob of {}", blob.display(), file.path));
        }
    }
    Ok((new.len(), reused))
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
    let matches = |file: &Artifact| matching(folder, file);
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
            ..Default::default()
        };
        let bytes = serde_json::to_vec_pretty(&manifest).unwrap();
        let key = SigningKey::from_bytes(&[7; 32]);
        let signature = base64().encode(key.sign(&bytes).to_bytes());
        let read = newen_update::verify(&bytes, &signature, &key.verifying_key()).unwrap();
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

    /// A bundle in `dir`: each (path, bytes).
    fn write_tree(dir: &Path, files: &[(&str, &[u8])]) {
        for (path, bytes) in files {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
    }

    /// The list of a release reuses every blob the release before published (read from its
    /// signed manifest, as `--previous` reads it) and every blob a platform before it packed:
    /// only the files that changed are written to upload. Its sequence is one more.
    #[test]
    fn a_release_reuses_earlier_blobs_and_uploads_only_what_changed() {
        let work = tempfile::tempdir().unwrap();
        let key = SigningKey::from_bytes(&[9; 32]);
        let base = |version: &str| {
            format!("https://github.com/Niiihuel/quill/releases/download/v{version}")
        };
        let (one, two) = (Version::new(1, 0, 0), Version::new(2, 0, 0));
        let tree = work.path().join("tree-1");
        write_tree(
            &tree,
            &[
                ("quill", b"\x7fELF one"),
                ("libcef.so", b"cef"),
                ("locales/en-US.pak", b"en"),
            ],
        );
        #[cfg(unix)]
        std::os::unix::fs::symlink("libcef.so", tree.join("libcef-link.so")).unwrap();
        let out = work.path().join("out-1");
        let written =
            build_files(&one, &base("1.0.0"), &out, &[], &[("linux-x86_64", &tree)]).unwrap();
        // The list and the three blobs: everything is new.
        assert_eq!(written.len(), 4, "{written:?}");
        let list = out.join(files::index_name("linux-x86_64"));
        let published = Manifest {
            version: one.clone(),
            sequence: 1,
            files: [(
                "linux-x86_64".to_owned(),
                served(&base("1.0.0"), &list, &files::index_name("linux-x86_64")).unwrap(),
            )]
            .into(),
            ..Default::default()
        };
        let bytes = serde_json::to_vec_pretty(&published).unwrap();
        let manifest_file = out.join(newen_update::MANIFEST);
        std::fs::write(&manifest_file, &bytes).unwrap();
        std::fs::write(
            signature_path(&manifest_file),
            base64().encode(key.sign(&bytes).to_bytes()),
        )
        .unwrap();
        // The one published, verified: each blob is in its folder and unpacks to its file.
        let read = read_signed(&manifest_file, &[key.verifying_key()]).unwrap();
        assert_eq!(
            check_files(&read, "linux-x86_64", &read.files["linux-x86_64"], &out),
            Ok((3, 0))
        );
        let (previous, lists) = read_previous(&out, &[key.verifying_key()]).unwrap();
        // Another key's release is not one to build on.
        assert!(read_previous(&out, &[SigningKey::from_bytes(&[1; 32]).verifying_key()]).is_err());

        // 2.0.0: the program changed; Windows' bundle shares the locale.
        let linux = work.path().join("tree-2");
        write_tree(
            &linux,
            &[
                ("quill", b"\x7fELF two"),
                ("libcef.so", b"cef"),
                ("locales/en-US.pak", b"en"),
            ],
        );
        let windows = work.path().join("tree-2w");
        write_tree(
            &windows,
            &[("quill.exe", b"MZ two"), ("locales/en-US.pak", b"en")],
        );
        let out = work.path().join("out-2");
        let written = build_files(
            &two,
            &base("2.0.0"),
            &out,
            &lists,
            &[("linux-x86_64", &linux), ("windows-x86_64", &windows)],
        )
        .unwrap();
        let blobs: Vec<_> = (written.iter())
            .filter(|file| file.extension().is_some_and(|ext| ext == "gz"))
            .collect();
        assert_eq!(blobs.len(), 2, "the two programs only: {written:?}");
        let list_of = |platform: &str| -> Bundle {
            serde_json::from_slice(&std::fs::read(out.join(files::index_name(platform))).unwrap())
                .unwrap()
        };
        let url_of = |bundle: &Bundle, path: &str| {
            bundle
                .files
                .iter()
                .find(|file| file.path == path)
                .unwrap()
                .url
                .clone()
        };
        let (linux, windows) = (list_of("linux-x86_64"), list_of("windows-x86_64"));
        assert!(url_of(&linux, "quill").starts_with(&base("2.0.0")));
        assert!(url_of(&linux, "libcef.so").starts_with(&base("1.0.0")));
        assert!(url_of(&windows, "quill.exe").starts_with(&base("2.0.0")));
        assert_eq!(
            url_of(&windows, "locales/en-US.pak"),
            url_of(&linux, "locales/en-US.pak")
        );
        let next = Manifest {
            version: two.clone(),
            files: [(
                "linux-x86_64".to_owned(),
                served(
                    &base("2.0.0"),
                    &out.join(files::index_name("linux-x86_64")),
                    &files::index_name("linux-x86_64"),
                )
                .unwrap(),
            )]
            .into(),
            ..Default::default()
        };
        assert_eq!(
            check_files(&next, "linux-x86_64", &next.files["linux-x86_64"], &out),
            Ok((1, 2))
        );

        // The sequence: one more; the same for the same version made again; never backwards.
        assert_eq!(sequence(&two, Some(&previous)), Ok(2));
        assert_eq!(sequence(&one, Some(&previous)), Ok(1));
        assert_eq!(sequence(&one, None), Ok(1));
        assert!(sequence(&Version::new(0, 9, 0), Some(&previous)).is_err());
    }
}
