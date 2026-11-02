//! Cisco's OpenH264 library, which plays the clips of What's new (docs/UPDATES.md). Cisco's
//! patent license covers it only when it is downloaded from Cisco onto this computer, with the
//! person's say: nothing is fetched until `Settings::video_codec` is yes. It is fetched from
//! Cisco over HTTPS, decompressed (bzip2), matched against the digest of the release limn-video
//! loads, and kept under the data folder; turned off, it is deleted.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

/// One platform's library in Cisco's release, as the host names it (limn-video's
/// `cisco::current()`): the engine names no codec of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodecRelease {
    pub version: String,
    /// The bzip2-compressed library on Cisco's server.
    pub url: String,
    pub file_name: String,
    /// The decompressed library's SHA-256, lowercase hex.
    pub sha256: String,
}

/// No compressed download, and no library, is larger (Cisco's are about 1 MB).
const MAX_COMPRESSED: u64 = 16 << 20;
const MAX_LIBRARY: u64 = 32 << 20;

/// Where `release` is kept under `folder`.
pub(crate) fn path(release: &CodecRelease, folder: &Path) -> PathBuf {
    folder
        .join("openh264")
        .join(&release.version)
        .join(&release.file_name)
}

/// The library kept under `folder`, if it is there and still matches.
pub(crate) fn kept(release: &CodecRelease, folder: &Path) -> Option<PathBuf> {
    let path = path(release, folder);
    let bytes = std::fs::read(&path).ok()?;
    (hex(&bytes) == release.sha256).then_some(path)
}

/// Downloads `release` from Cisco into `folder`; what does not match is never written.
pub(crate) fn fetch(release: &CodecRelease, folder: &Path) -> Result<PathBuf, String> {
    if !release.url.starts_with("https://") {
        return Err("the codec's address is not HTTPS".into());
    }
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .https_only(true)
            .timeout_global(Some(Duration::from_secs(120)))
            .build(),
    );
    let mut response = agent
        .get(&release.url)
        .call()
        .map_err(|error| format!("Could not reach Cisco's server: {error}"))?;
    let compressed = response
        .body_mut()
        .with_config()
        .limit(MAX_COMPRESSED)
        .read_to_vec()
        .map_err(|error| format!("The codec's download did not finish: {error}"))?;
    install(&compressed, release, folder)
}

/// `compressed` decompressed into `folder` as `release`'s library, once it is the library's
/// digest; written beside and renamed, so a half-written library is never there.
pub(crate) fn install(
    compressed: &[u8],
    release: &CodecRelease,
    folder: &Path,
) -> Result<PathBuf, String> {
    let mut library = vec![];
    bzip2::read::BzDecoder::new(compressed)
        .take(MAX_LIBRARY + 1)
        .read_to_end(&mut library)
        .map_err(|error| format!("The codec's download is not bzip2: {error}"))?;
    if library.len() as u64 > MAX_LIBRARY || hex(&library) != release.sha256 {
        return Err("The codec downloaded is not Cisco's release: it was not kept.".into());
    }
    let path = path(release, folder);
    let parent = path.parent().expect("a folder");
    let disk = |error: std::io::Error| format!("Could not keep the codec: {error}");
    std::fs::create_dir_all(parent).map_err(disk)?;
    let part = parent.join(format!(".{}.part", release.file_name));
    let mut file = std::fs::File::create(&part).map_err(disk)?;
    file.write_all(&library).map_err(disk)?;
    file.sync_all().map_err(disk)?;
    std::fs::rename(&part, &path).map_err(disk)?;
    Ok(path)
}

/// Every codec kept under `folder` goes: the person turned it off.
pub(crate) fn remove(folder: &Path) {
    let _ = std::fs::remove_dir_all(folder.join("openh264"));
}

fn hex(bytes: &[u8]) -> String {
    newen_update::hex(&Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bz2(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn release(library: &[u8]) -> CodecRelease {
        CodecRelease {
            version: "2.6.0".into(),
            url: "https://ciscobinary.openh264.org/libopenh264-2.6.0-linux64.8.so.bz2".into(),
            file_name: "libopenh264-2.6.0-linux64.8.so".into(),
            sha256: hex(library),
        }
    }

    /// The library is kept when it is the release's; anything else is refused and nothing is
    /// written, not even the part.
    #[test]
    fn a_codec_is_kept_only_when_it_is_ciscos_release() {
        let folder = tempfile::tempdir().unwrap();
        let wanted = release(b"the library");
        assert_eq!(
            install(&bz2(b"another library"), &wanted, folder.path()),
            Err("The codec downloaded is not Cisco's release: it was not kept.".into())
        );
        assert!(install(b"not bzip2", &wanted, folder.path()).is_err());
        assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 0);
        assert_eq!(kept(&wanted, folder.path()), None);
        let path = install(&bz2(b"the library"), &wanted, folder.path()).unwrap();
        assert_eq!(
            path,
            folder
                .path()
                .join("openh264/2.6.0/libopenh264-2.6.0-linux64.8.so")
        );
        assert_eq!(kept(&wanted, folder.path()), Some(path.clone()));
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(names.len(), 1);
        // A file changed on the disk no longer counts.
        std::fs::write(&path, b"changed").unwrap();
        assert_eq!(kept(&wanted, folder.path()), None);
        remove(folder.path());
        assert!(!path.exists());
        // Never from anywhere but HTTPS.
        let plain = CodecRelease {
            url: "http://ciscobinary.openh264.org/x.bz2".into(),
            ..wanted
        };
        assert!(fetch(&plain, folder.path()).is_err());
    }

    /// Cisco's own download decompresses to its library: run with `LIMN_VIDEO_OPENH264` set to
    /// the library, with the `.bz2` Cisco serves beside it; skipped without.
    #[test]
    fn ciscos_download_decompresses_to_its_library() {
        let Some(library) = std::env::var_os("LIMN_VIDEO_OPENH264") else {
            eprintln!("skipped: set LIMN_VIDEO_OPENH264 to Cisco's library, its .bz2 beside it");
            return;
        };
        let library = PathBuf::from(library);
        let Ok(compressed) = std::fs::read(format!("{}.bz2", library.display())) else {
            eprintln!("skipped: no .bz2 beside {}", library.display());
            return;
        };
        let wanted = release(&std::fs::read(&library).unwrap());
        let folder = tempfile::tempdir().unwrap();
        let path = install(&compressed, &wanted, folder.path()).unwrap();
        assert_eq!(kept(&wanted, folder.path()), Some(path));
    }
}
