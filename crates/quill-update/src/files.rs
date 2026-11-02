//! A release's bundle file by file (docs/UPDATES.md): `files-<platform>.json`, which the signed
//! manifest names by size and SHA-256, lists every file of the bundle as the release's archive
//! holds it (path, size, SHA-256, mode) with its gzipped copy, a blob named by its digest
//! (`f-<sha256>.gz`), and the symbolic links of macOS' framework. A blob an earlier release
//! published is named at that release's address, so a release uploads only the files that
//! changed, and a copy downloads only the files it does not have: the rest it takes from the
//! version that runs. Any failure leaves the full archive to fall back on.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::install::Install;
use crate::{disk, fetch, fetch_to, hex, sha256, Artifact, Error, Source, Version};

/// No list of files is larger.
pub const MAX_INDEX: u64 = 4 << 20;
/// No bundle has more files.
const MAX_FILES: usize = 20_000;

/// The list's name among a release's files.
pub fn index_name(platform: &str) -> String {
    format!("files-{platform}.json")
}

/// A blob's name among a release's files: the digest of what it holds once unpacked.
pub fn blob_name(sha256: &str) -> String {
    format!("f-{sha256}.gz")
}

/// Every file of one platform's bundle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bundle {
    pub version: Version,
    pub platform: String,
    pub files: Vec<File>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Link>,
}

/// One file, and where its blob is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
    /// Relative, `/` between folders, as the archive holds it (macOS: `quill.app/…`).
    pub path: String,
    pub size: u64,
    /// Of the file itself, lowercase hex.
    pub sha256: String,
    /// Its Unix permissions (`0o755`); Windows ignores them.
    pub mode: u32,
    /// Its blob, maybe an earlier release's.
    pub url: String,
    /// The blob's size.
    pub packed: u64,
}

/// A symbolic link, its target relative to its folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub path: String,
    pub target: String,
}

/// The list `index` names, for `version` on `platform`, once its size and digest match what
/// was signed and every path in it stays inside the bundle.
pub fn fetch_bundle(
    agent: &ureq::Agent,
    source: &Source,
    index: &Artifact,
    version: &Version,
    platform: &str,
) -> Result<Bundle, Error> {
    let bytes = fetch(agent, &index.url, source.grant(), MAX_INDEX as usize)?;
    if bytes.len() as u64 != index.size
        || !hex(&Sha256::digest(&bytes)).eq_ignore_ascii_case(&index.sha256)
    {
        return Err(Error::Digest);
    }
    let bundle: Bundle = serde_json::from_slice(&bytes).map_err(|error| {
        eprintln!("quill: update: unreadable list of files: {error}");
        Error::Manifest
    })?;
    if bundle.version != *version || bundle.platform != platform || !valid(&bundle) {
        return Err(Error::Manifest);
    }
    Ok(bundle)
}

/// Every path is a relative one inside the bundle, named once; every link points inside it;
/// sizes, digests and addresses are ones quill downloads.
pub fn valid(bundle: &Bundle) -> bool {
    let mut seen = std::collections::HashSet::new();
    let mut paths = (bundle.files.iter().map(|file| &file.path))
        .chain(bundle.links.iter().map(|link| &link.path));
    let unique = paths.all(|path| relative(path) && seen.insert(path));
    let files = bundle.files.iter().all(|file| {
        crate::usable(&Artifact {
            url: file.url.clone(),
            size: file.packed,
            sha256: file.sha256.clone(),
        }) && file.size <= crate::MAX_ARTIFACT
    });
    let links = (bundle.links.iter()).all(|link| inside(&link.path, &link.target));
    bundle.files.len() + bundle.links.len() <= MAX_FILES && unique && files && links
}

/// `path` is relative and plain: folders by `/`, none empty, `.` or `..`, and nothing Windows
/// reads as a drive or a stream.
fn relative(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':', '\0'])
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

/// A link at `path` to `target` stays inside the bundle.
fn inside(path: &str, target: &str) -> bool {
    if !relative(path)
        || target.is_empty()
        || target.starts_with('/')
        || target.contains(['\\', ':', '\0'])
    {
        return false;
    }
    let mut depth = path.split('/').count() as i64 - 1;
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => depth -= 1,
            _ => depth += 1,
        }
        if depth < 0 {
            return false;
        }
    }
    true
}

/// `path` (`/` between folders) under `root`.
fn under(root: &Path, path: &str) -> PathBuf {
    path.split('/')
        .fold(root.to_path_buf(), |at, part| at.join(part))
}

/// Stage `bundle` in `install`: each file the version that runs has with the same digest is
/// taken from it (a hard link on Linux when its mode is the same, else a copy), the others are
/// downloaded as blobs into `blobs` (each once, resumed when a part is there, no larger than
/// listed) and unpacked, each checked against its digest; then modes are set and links made.
/// `progress` hears the bytes downloaded and the whole to download, and stops it by answering
/// false. Returns where it is staged. A failure leaves nothing staged: the full archive is
/// the way left.
pub fn stage(
    agent: &ureq::Agent,
    source: &Source,
    install: &Install,
    bundle: &Bundle,
    blobs: &Path,
    mut progress: impl FnMut(u64, u64) -> bool,
) -> Result<PathBuf, Error> {
    let mut local: HashMap<&str, PathBuf> = HashMap::new();
    for file in &bundle.files {
        if local.contains_key(file.sha256.as_str()) {
            continue;
        }
        let path = install.installed(&file.path);
        let same = fs::metadata(&path)
            .is_ok_and(|found| found.is_file() && found.len() == file.size)
            && sha256(&path).is_ok_and(|found| found == file.sha256);
        if same {
            local.insert(&file.sha256, path);
        }
    }
    let wanted: BTreeMap<&str, &File> = (bundle.files.iter())
        .filter(|file| !local.contains_key(file.sha256.as_str()))
        .map(|file| (file.sha256.as_str(), file))
        .collect();
    fs::create_dir_all(blobs).map_err(disk)?;
    // What another release left goes.
    for entry in fs::read_dir(blobs).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !wanted.keys().any(|sha| name.starts_with(&blob_name(sha))) {
            let _ = fs::remove_file(entry.path());
        }
    }
    let missing: Vec<&File> = (wanted.values().copied())
        .filter(|file| !blobs.join(blob_name(&file.sha256)).is_file())
        .collect();
    let total = missing.iter().map(|file| file.packed).sum();
    let mut done = 0;
    for file in missing {
        let blob = blobs.join(blob_name(&file.sha256));
        let part = blobs.join(format!("{}.part", blob_name(&file.sha256)));
        let before = done;
        fetch_to(
            agent,
            source,
            &file.url,
            file.packed,
            &part,
            &mut |have, _| progress(before + have, total),
        )?;
        fs::rename(&part, &blob).map_err(disk)?;
        done += file.packed;
    }
    progress(total, total);
    let staged = install
        .stage_with(&bundle.version, |staging| {
            assemble(bundle, &local, blobs, staging)
        })
        .map_err(disk)?;
    let _ = fs::remove_dir_all(blobs);
    Ok(staged)
}

/// The bundle built in the empty folder `staging` from the files `local` has and the blobs.
fn assemble(
    bundle: &Bundle,
    local: &HashMap<&str, PathBuf>,
    blobs: &Path,
    staging: &Path,
) -> io::Result<()> {
    for file in &bundle.files {
        let to = under(staging, &file.path);
        if let Some(folder) = to.parent() {
            fs::create_dir_all(folder)?;
        }
        match local.get(file.sha256.as_str()) {
            Some(from) if link(from, &to, file.mode) => continue,
            Some(from) => {
                fs::copy(from, &to)?;
            }
            None => unpack(&blobs.join(blob_name(&file.sha256)), &to, file)?,
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&to, fs::Permissions::from_mode(file.mode & 0o777))?;
        }
    }
    #[cfg(not(unix))]
    if !bundle.links.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "links in a bundle for this system",
        ));
    }
    #[cfg(unix)]
    for link in &bundle.links {
        let at = under(staging, &link.path);
        if let Some(folder) = at.parent() {
            fs::create_dir_all(folder)?;
        }
        std::os::unix::fs::symlink(&link.target, &at)?;
    }
    // A bundle that is not exactly the one signed would not open: the full archive then.
    #[cfg(target_os = "macos")]
    crate::install::run(
        std::process::Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(staging.join("quill.app")),
    )?;
    Ok(())
}

/// Linux: `to` becomes another name of `from` when `from` has `mode` already, so the versions
/// share it on the disk; neither is ever written again. Whether it did.
fn link(from: &Path, to: &Path, mode: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        let same = fs::metadata(from)
            .is_ok_and(|found| found.permissions().mode() & 0o7777 == mode & 0o777);
        same && fs::hard_link(from, to).is_ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (from, to, mode);
        false
    }
}

/// The blob at `blob` unpacked into `to`, no larger than `file` says, and only when it is that
/// file; else the blob goes, so a retry downloads it again.
fn unpack(blob: &Path, to: &Path, file: &File) -> io::Result<()> {
    let mut reader =
        flate2::read::GzDecoder::new(io::BufReader::new(fs::File::open(blob)?)).take(file.size + 1);
    let mut out = fs::File::create(to)?;
    let mut hasher = Sha256::new();
    let mut written = 0;
    let mut chunk = vec![0; 1 << 16];
    let unpacked = loop {
        match reader.read(&mut chunk) {
            Ok(0) => break Ok(()),
            Ok(read) => {
                hasher.update(&chunk[..read]);
                out.write_all(&chunk[..read])?;
                written += read as u64;
            }
            Err(error) => break Err(error),
        }
    };
    if unpacked.is_err() || written != file.size || hex(&hasher.finalize()) != file.sha256 {
        let _ = fs::remove_file(blob);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: its blob is not the file the release names", file.path),
        ));
    }
    Ok(())
}

/// The blobs earlier releases published, by digest: their address and size.
pub type Known = HashMap<String, (String, u64)>;

/// What `bundles`, earlier releases' lists, published.
pub fn known(bundles: &[Bundle]) -> Known {
    (bundles.iter().flat_map(|bundle| &bundle.files))
        .map(|file| (file.sha256.clone(), (file.url.clone(), file.packed)))
        .collect()
}

#[cfg(any(test, feature = "serve"))]
/// The list of the bundle in the folder `tree`, as `version` on `platform` publishes it at
/// `base`: a file `known` names keeps its blob there; any other is gzipped into `out` as its
/// blob. Returns the list and the blobs written, which are all the release uploads of it.
/// Folders are left out (each file's path makes its own), and so is `__MACOSX/` at the top,
/// where an unzip puts what macOS' zip keeps of a file's attributes.
pub fn pack(
    tree: &Path,
    version: &Version,
    platform: &str,
    base: &str,
    known: &Known,
    out: &Path,
) -> io::Result<(Bundle, Vec<PathBuf>)> {
    let mut bundle = Bundle {
        version: version.clone(),
        platform: platform.to_owned(),
        files: Vec::new(),
        links: Vec::new(),
    };
    let mut written = Vec::new();
    let mut packed: Known = HashMap::new();
    fs::create_dir_all(out)?;
    let mut pending = vec![(tree.to_path_buf(), String::new())];
    while let Some((folder, prefix)) = pending.pop() {
        let mut entries: Vec<_> = fs::read_dir(&folder)?.collect::<io::Result<_>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = format!("{prefix}{name}");
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                let target = fs::read_link(entry.path())?
                    .to_string_lossy()
                    .replace('\\', "/");
                bundle.links.push(Link { path, target });
            } else if kind.is_dir() {
                if path != "__MACOSX" {
                    pending.push((entry.path(), format!("{path}/")));
                }
            } else {
                let bytes = fs::read(entry.path())?;
                let sha256 = hex(&Sha256::digest(&bytes));
                let (url, size) = match known.get(&sha256).or_else(|| packed.get(&sha256)) {
                    Some(blob) => blob.clone(),
                    None => {
                        let name = blob_name(&sha256);
                        let mut encoder = flate2::write::GzEncoder::new(
                            Vec::new(),
                            flate2::Compression::default(),
                        );
                        encoder.write_all(&bytes)?;
                        let blob = encoder.finish()?;
                        fs::write(out.join(&name), &blob)?;
                        written.push(out.join(&name));
                        let entry = (
                            format!("{}/{name}", base.trim_end_matches('/')),
                            blob.len() as u64,
                        );
                        packed.insert(sha256.clone(), entry.clone());
                        entry
                    }
                };
                bundle.files.push(File {
                    path,
                    size: bytes.len() as u64,
                    sha256,
                    mode: mode(&entry.metadata()?),
                    url,
                    packed: size,
                });
            }
        }
    }
    bundle.files.sort_by(|a, b| a.path.cmp(&b.path));
    bundle.links.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((bundle, written))
}

#[cfg(all(unix, any(test, feature = "serve")))]
fn mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o777
}

#[cfg(all(not(unix), any(test, feature = "serve")))]
fn mode(_: &fs::Metadata) -> u32 {
    0o644
}
