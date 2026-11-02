use base64::Engine as _;
use ed25519_dalek::{Signer, SigningKey};

use super::*;
#[cfg(target_os = "linux")]
use crate::install::Install;
use crate::serve::{serve, Server};

fn key() -> SigningKey {
    let mut seed = [0; 32];
    getrandom::fill(&mut seed).unwrap();
    SigningKey::from_bytes(&seed)
}

fn sign(key: &SigningKey, bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(key.sign(bytes).to_bytes())
}

fn version(text: &str) -> Version {
    Version::parse(text).unwrap()
}

fn artifact(url: &str, bytes: &[u8]) -> Artifact {
    Artifact {
        url: url.into(),
        size: bytes.len() as u64,
        sha256: hex(&Sha256::digest(bytes)),
    }
}

fn manifest(at: &str, minimum: Option<&str>, platforms: &[(&str, Artifact)]) -> Manifest {
    Manifest {
        version: version(at),
        date: "2026-10-19T12:00:00Z".into(),
        notes: "mandatory: fixes the startup".into(),
        minimum_version: minimum.map(version),
        platforms: platforms
            .iter()
            .map(|(name, artifact)| ((*name).to_owned(), artifact.clone()))
            .collect(),
        ..Default::default()
    }
}

#[test]
fn a_signed_manifest_verifies_and_any_change_or_other_key_is_refused() {
    let signer = key();
    let bytes = serde_json::to_vec(&manifest("1.2.0", None, &[])).unwrap();
    let signature = sign(&signer, &bytes);
    let public = signer.verifying_key();
    assert_eq!(
        verify(&bytes, &format!("{signature}\n"), &public)
            .unwrap()
            .version,
        version("1.2.0")
    );
    // One byte changed after signing.
    let mut altered = bytes.clone();
    let at = altered.iter().position(|byte| *byte == b'2').unwrap();
    altered[at] = b'9';
    assert_eq!(verify(&altered, &signature, &public), Err(Error::Signature));
    // Signed by someone else.
    assert_eq!(
        verify(&bytes, &sign(&key(), &bytes), &public),
        Err(Error::Signature)
    );
    assert_eq!(
        verify(&bytes, "no es base64", &public),
        Err(Error::Signature)
    );
    assert_eq!(verify(&bytes, "", &public), Err(Error::Signature));
    // A signature of bytes that are not a manifest proves nothing useful.
    let garbage = b"{\"version\":";
    assert_eq!(
        verify(garbage, &sign(&signer, garbage), &public),
        Err(Error::Manifest)
    );
}

#[test]
fn keys_are_base64_and_a_weak_one_is_refused() {
    let signer = key();
    let text = base64::engine::general_purpose::STANDARD.encode(signer.verifying_key().to_bytes());
    assert_eq!(
        parse_key(&format!(" {text}\n")),
        Some(signer.verifying_key())
    );
    assert_eq!(parse_key(""), None);
    assert_eq!(parse_key("AAAA"), None);
    // The identity point: a small-order key anyone can forge for.
    let mut identity = [0; 32];
    identity[0] = 1;
    assert_eq!(
        parse_key(&base64::engine::general_purpose::STANDARD.encode(identity)),
        None
    );
}

#[test]
fn a_newer_release_is_offered_mandatory_only_below_its_minimum() {
    let linux = artifact("https://example.org/quill-x86_64-linux.tar.gz", b"quill");
    let release = manifest("1.2.0", Some("1.1.0"), &[("linux-x86_64", linux.clone())]);
    let offered = |installed: &str| offer(release.clone(), &version(installed), "linux-x86_64");
    assert_eq!(offered("1.2.0"), None);
    assert_eq!(offered("1.3.0"), None);
    let below = offered("1.0.9").unwrap();
    assert!(below.mandatory);
    assert_eq!(below.artifact, Some(linux.clone()));
    assert!(!offered("1.1.0").unwrap().mandatory);
    assert!(!offered("1.1.5").unwrap().mandatory);
    // A pre-release of the minimum is older than it.
    assert!(offered("1.1.0-rc.1").unwrap().mandatory);
    // Without a minimum nothing is mandatory.
    let free = manifest("1.2.0", None, &[("linux-x86_64", linux.clone())]);
    assert!(
        !offer(free, &version("0.1.0"), "linux-x86_64")
            .unwrap()
            .mandatory
    );
    // Nothing for this platform: offered, without an artifact.
    let other = offer(release.clone(), &version("1.0.0"), "windows-x86_64").unwrap();
    assert!(other.mandatory && other.artifact.is_none());
    // An artifact that could not be trusted to install counts as none.
    for bad in [
        Artifact {
            url: "http://example.org/quill.tar.gz".into(),
            ..linux.clone()
        },
        Artifact {
            size: MAX_ARTIFACT + 1,
            ..linux.clone()
        },
        Artifact {
            sha256: "abc".into(),
            ..linux.clone()
        },
    ] {
        let release = manifest("1.2.0", None, &[("linux-x86_64", bad)]);
        assert_eq!(
            offer(release, &version("1.0.0"), "linux-x86_64")
                .unwrap()
                .artifact,
            None
        );
    }
}

#[test]
fn the_minimum_is_the_newest_mandatory_tag_up_to_the_release() {
    let tags = [
        ("v0.9.0", "mandatory: the service changed"),
        ("v1.0.0", "First public version"),
        ("v1.1.0", "Mandatory. Fixes the sign-in"),
        ("v1.2.0", "Notes about the earlier mandatories"),
        ("v1.3.0", "MANDATORY"),
        ("v2.0.0-rc.1", "mandatory (candidate)"),
        ("nightly", "mandatory"),
        ("1.4.0", "mandatory without the v"),
    ];
    let minimum = |at: &str| minimum_version(&version(at), tags.iter().copied());
    assert_eq!(minimum("1.2.0"), Some(version("1.1.0")));
    assert_eq!(minimum("1.3.0"), Some(version("1.3.0")));
    assert_eq!(minimum("1.0.0"), Some(version("0.9.0")));
    assert_eq!(minimum("2.0.0"), Some(version("2.0.0-rc.1")));
    assert_eq!(minimum("0.8.0"), None);
    assert!(mandatory("A mandatory version."));
    assert!(!mandatory("mandatories"));
    assert!(!mandatory("no-mandatorily"));
}

/// Publish `manifest`, signed by `signer`, on `server`.
fn publish(server: &Server, signer: &SigningKey, manifest: &Manifest) {
    publish_as(server, signer, manifest, MANIFEST);
}

/// [`publish`] under the manifest's `name`, its signature beside it.
fn publish_as(server: &Server, signer: &SigningKey, manifest: &Manifest, name: &str) {
    let bytes = serde_json::to_vec_pretty(manifest).unwrap();
    server.put(&format!("/{name}.sig"), sign(signer, &bytes).into_bytes());
    server.put(&format!("/{name}"), bytes);
}

/// The paths `server` was asked for since `from`.
fn asked(server: &Server, from: usize) -> Vec<String> {
    server.requests()[from..]
        .iter()
        .map(|(path, _)| path.clone())
        .collect()
}

/// At a feed, Newen's manifest is looked for first and quill's when there is none; either goes
/// through the same signature and admission, and Newen's may name itself the product.
#[test]
fn a_feed_s_newen_manifest_is_read_first_and_quill_s_when_there_is_none() {
    let signer = key();
    let server = serve(0).unwrap();
    let (agent, feed) = (agent(), Source::at(server.base()));
    let trust = Trust::new(vec![signer.verifying_key()]);
    let quill = manifest("9.0.0", None, &[]);
    publish(&server, &signer, &quill);
    assert_eq!(check(&agent, &feed, &trust), Ok(quill.clone()));
    assert_eq!(
        asked(&server, 0),
        [
            "/newen-update.json",
            "/quill-update.json",
            "/quill-update.json.sig",
            "/quill-update.json.sigs"
        ]
    );
    let newen = Manifest {
        product: NEWEN_PRODUCT.into(),
        ..manifest("9.1.0", None, &[])
    };
    publish_as(&server, &signer, &newen, NEWEN_MANIFEST);
    let before = server.requests().len();
    assert_eq!(check(&agent, &feed, &trust), Ok(newen.clone()));
    assert_eq!(
        asked(&server, before),
        [
            "/newen-update.json",
            "/newen-update.json.sig",
            "/newen-update.json.sigs"
        ]
    );
    // Newen's, signed by another key: refused, and quill's is not read instead.
    publish_as(&server, &key(), &newen, NEWEN_MANIFEST);
    assert_eq!(check(&agent, &feed, &trust), Err(Error::Signature));
    // Nor another product's under Newen's name.
    let other = Manifest {
        product: "other".into(),
        ..newen
    };
    publish_as(&server, &signer, &other, NEWEN_MANIFEST);
    assert_eq!(check(&agent, &feed, &trust), Err(Error::Foreign));
}

/// The releases are read from Newen's repository first and from quill's when Newen's has no
/// manifest (before the rename); only a 404 moves on. A test feed is one base alone.
#[test]
fn newen_s_repository_is_read_first_and_quill_s_when_it_has_no_manifest() {
    assert_eq!(
        Source::from_feed(None).bases,
        [
            "https://github.com/Niiihuel/newen/releases/latest/download",
            "https://github.com/Niiihuel/quill/releases/latest/download"
        ]
    );
    assert_eq!(
        Source::from_feed(Some("  ".into())),
        Source::from_feed(None)
    );
    assert_eq!(
        Source::from_feed(Some("http://127.0.0.1:8765/".into())),
        Source::at("http://127.0.0.1:8765")
    );

    let signer = key();
    let trust = Trust::new(vec![signer.verifying_key()]);
    let (renamed, old) = (serve(0).unwrap(), serve(0).unwrap());
    let both = Source {
        bases: vec![renamed.base(), old.base()],
    };
    let published = manifest("9.0.0", None, &[]);
    publish(&old, &signer, &published);
    assert_eq!(check(&agent(), &both, &trust), Ok(published));
    assert_eq!(
        asked(&renamed, 0),
        ["/newen-update.json", "/quill-update.json"]
    );
    // Once the first has a release, the second is not asked.
    let newer = manifest("9.1.0", None, &[]);
    publish(&renamed, &signer, &newer);
    let before = old.requests().len();
    assert_eq!(check(&agent(), &both, &trust), Ok(newer));
    assert_eq!(old.requests().len(), before);
    // A first base that does not answer is the answer: no fallback hides it.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let nobody = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let unreachable = Source {
        bases: vec![nobody, old.base()],
    };
    assert_eq!(check(&agent(), &unreachable, &trust), Err(Error::Network));
}

/// What copies up to 0.1.8 read of a release stays what 0.1.9 publishes: they look its
/// manifest and signature up by these names, and install only a bundle with this program at
/// its root (`tools/quill-release`, `scripts/package.sh` and the workflows are unchanged).
#[test]
fn what_copies_up_to_0_1_8_read_of_a_release_is_unchanged() {
    assert_eq!(MANIFEST, "quill-update.json");
    assert_eq!(SIGNATURE, "quill-update.json.sig");
    assert_eq!(
        install::PROGRAM,
        if cfg!(windows) { "quill.exe" } else { "quill" }
    );
    // The local test feed publishes under them, and a check still finds them.
    let server = serve(0).unwrap();
    let published = manifest("9.0.0", None, &[]);
    let key = serve::publish(&server, &published);
    assert_eq!(
        check(&agent(), &Source::at(server.base()), &Trust::new(vec![key])),
        Ok(published)
    );
}

#[test]
fn a_check_reads_a_signed_feed_on_loopback_and_refuses_a_forged_one() {
    let signer = key();
    let release = manifest("9.0.0", Some("9.0.0"), &[]);
    let server = serve(0).unwrap();
    let agent = agent();
    let feed = Source::at(server.base());
    let public = signer.verifying_key();
    // Nothing there yet.
    assert_eq!(
        check(&agent, &feed, &Trust::new(vec![public])),
        Err(Error::Status(404))
    );
    publish(&server, &signer, &release);
    assert_eq!(
        check(&agent, &feed, &Trust::new(vec![public])),
        Ok(release.clone())
    );
    assert_eq!(check(&agent, &feed, &Trust::default()), Err(Error::NoKey));
    // The same feed, verified with another key.
    assert_eq!(
        check(&agent, &feed, &Trust::new(vec![key().verifying_key()])),
        Err(Error::Signature)
    );
    // A manifest changed after signing.
    server.put(
        &format!("/{MANIFEST}"),
        serde_json::to_vec_pretty(&manifest("9.0.1", None, &[])).unwrap(),
    );
    assert_eq!(
        check(&agent, &feed, &Trust::new(vec![public])),
        Err(Error::Signature)
    );
    // A manifest larger than any real one.
    server.put(&format!("/{MANIFEST}"), vec![b' '; MAX_MANIFEST + 1]);
    assert_eq!(
        check(&agent, &feed, &Trust::new(vec![public])),
        Err(Error::TooLarge)
    );
}

fn program(size: usize) -> Vec<u8> {
    (0..size).map(|at| (at * 31 % 251) as u8).collect()
}

/// A server with `body` at `/quill.gz`, and its artifact.
fn artifact_server(body: &[u8]) -> (Server, Artifact) {
    let server = serve(0).unwrap();
    server.put("/quill.gz", body.to_vec());
    let wanted = artifact(&format!("{}/quill.gz", server.base()), body);
    (server, wanted)
}

#[test]
fn a_download_is_kept_only_when_its_size_and_digest_match() {
    let body = program(300_000);
    let (server, good) = artifact_server(&body);
    let agent = agent();
    let folder = tempfile::tempdir().unwrap();
    let mut seen = Vec::new();
    let path = download(&agent, &good, folder.path(), |have, whole| {
        seen.push((have, whole));
        true
    })
    .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), body);
    assert_eq!(seen.first(), Some(&(0, body.len() as u64)));
    assert_eq!(seen.last(), Some(&(body.len() as u64, body.len() as u64)));
    // Asked again (its installation failed): there already, not fetched.
    let before = server.requests().len();
    assert_eq!(
        download(&agent, &good, folder.path(), |_, _| true),
        Ok(path.clone())
    );
    assert_eq!(server.requests().len(), before);

    // Another digest: what came is deleted.
    let mut wrong = good.clone();
    wrong.sha256 = hex(&Sha256::digest(b"otra cosa"));
    assert_eq!(
        download(&agent, &wrong, folder.path(), |_, _| true),
        Err(Error::Digest)
    );
    // The earlier release's bundle is not kept beside it.
    assert!(!path.exists());
    assert!(!folder
        .path()
        .join(format!("quill-{}.part", wrong.sha256))
        .exists());
    assert!(!folder
        .path()
        .join(format!("quill-{}", wrong.sha256))
        .exists());
    // Larger than signed: stopped and deleted.
    let mut short = wrong.clone();
    short.size = 1000;
    assert_eq!(
        download(&agent, &short, folder.path(), |_, _| true),
        Err(Error::Digest)
    );
    assert!(!folder
        .path()
        .join(format!("quill-{}.part", short.sha256))
        .exists());
    // Smaller than signed: kept to resume, never installed.
    let mut long = wrong;
    long.size = body.len() as u64 + 10;
    assert_eq!(
        download(&agent, &long, folder.path(), |_, _| true),
        Err(Error::Incomplete)
    );
}

#[test]
fn a_cut_download_resumes_where_it_stopped() {
    let body = program(400_000);
    let (server, wanted) = artifact_server(&body);
    server.configure(|files| files.cut = Some(("/quill.gz".into(), 150_000)));
    let agent = agent();
    let folder = tempfile::tempdir().unwrap();
    let first = download(&agent, &wanted, folder.path(), |_, _| true);
    assert!(
        matches!(first, Err(Error::Network | Error::Incomplete)),
        "{first:?}"
    );
    let part = folder.path().join(format!("quill-{}.part", wanted.sha256));
    // What arrived before the cut, as far as the client read it.
    let kept = std::fs::metadata(&part).unwrap().len();
    assert!(kept > 0 && kept <= 150_000, "{kept}");
    let path = download(&agent, &wanted, folder.path(), |_, _| true).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), body);
    assert_eq!(
        server.requests(),
        vec![("/quill.gz".into(), None), ("/quill.gz".into(), Some(kept))]
    );
}

/// A download its progress stops ends there, its part kept, and the next one resumes it.
#[test]
fn a_stopped_download_keeps_its_part() {
    let body = program(400_000);
    let (_server, wanted) = artifact_server(&body);
    let agent = agent();
    let folder = tempfile::tempdir().unwrap();
    let stopped = download(&agent, &wanted, folder.path(), |have, _| have < 100_000);
    assert_eq!(stopped, Err(Error::Interrupted));
    let part = folder.path().join(format!("quill-{}.part", wanted.sha256));
    assert!(std::fs::metadata(&part).unwrap().len() >= 100_000);
    let path = download(&agent, &wanted, folder.path(), |_, _| true).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), body);
}

#[test]
fn a_server_without_ranges_sends_it_again_from_the_start() {
    let body = program(200_000);
    let (server, wanted) = artifact_server(&body);
    server.configure(|files| {
        files.cut = Some(("/quill.gz".into(), 50_000));
        files.no_ranges = true;
    });
    let agent = agent();
    let folder = tempfile::tempdir().unwrap();
    assert!(download(&agent, &wanted, folder.path(), |_, _| true).is_err());
    let path = download(&agent, &wanted, folder.path(), |_, _| true).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), body);
}

#[test]
fn a_program_is_known_by_its_first_bytes_and_a_bundle_by_its_path() {
    use crate::install::{bundle_root, starts_native};
    use std::path::Path;

    assert!(starts_native("linux", b"\x7fELF\x02"));
    assert!(starts_native("windows", b"MZ\x90\x00"));
    assert!(starts_native("macos", &[0xcf, 0xfa, 0xed, 0xfe, 0x0c]));
    assert!(starts_native("macos", &[0xca, 0xfe, 0xba, 0xbe]));
    // Each system refuses the others' programs, and an archive.
    assert!(!starts_native("linux", b"MZ\x90\x00"));
    assert!(!starts_native("windows", b"\x7fELF"));
    assert!(!starts_native("macos", b"\x7fELF"));
    assert!(!starts_native("linux", b"quill\0\0\0"));
    assert!(!starts_native("linux", b""));

    let program = Path::new("/Users/ana/Applications/quill.app/Contents/MacOS/quill");
    assert_eq!(
        bundle_root(program),
        Some(Path::new("/Users/ana/Applications"))
    );
    assert_eq!(
        bundle_root(Path::new("/Applications/quill.app/Contents/MacOS/quill")),
        Some(Path::new("/Applications"))
    );
    // Not in a bundle, or in one of another name.
    assert_eq!(
        bundle_root(Path::new("/src/quill/target/debug/quill")),
        None
    );
    assert_eq!(
        bundle_root(Path::new("/Applications/Other.app/Contents/MacOS/quill")),
        None
    );
    assert_eq!(bundle_root(Path::new("quill")), None);
}

/// A tiny "program": the ELF magic and a marker.
#[cfg(target_os = "linux")]
fn elf(marker: &str) -> Vec<u8> {
    [b"\x7fELF".as_slice(), marker.as_bytes()].concat()
}

/// A bundle as a release holds it, in `dir`: `quill` (an executable starting with `program`),
/// CEF's library and a locale, each saying `marker`.
#[cfg(target_os = "linux")]
fn bundle(dir: &std::path::Path, program: &[u8], marker: &str) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::create_dir_all(dir.join("locales")).unwrap();
    std::fs::write(dir.join("quill"), program).unwrap();
    std::fs::set_permissions(dir.join("quill"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(dir.join("libcef.so"), format!("cef {marker}")).unwrap();
    std::fs::write(dir.join("locales/en-US.pak"), format!("en-US {marker}")).unwrap();
}

/// The Linux artifact of a bundle in `dir`: a gzipped tar, flat at its root.
#[cfg(target_os = "linux")]
fn tar_gz(dir: &std::path::Path, out: &std::path::Path) -> Vec<u8> {
    tar_gz_of(dir, out, &["quill", "libcef.so", "locales"])
}

/// [`tar_gz`] of the entries `names` of `dir`.
#[cfg(target_os = "linux")]
fn tar_gz_of(dir: &std::path::Path, out: &std::path::Path, names: &[&str]) -> Vec<u8> {
    let status = std::process::Command::new("tar")
        .arg("-czf")
        .arg(out)
        .arg("-C")
        .arg(dir)
        .args(names)
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::read(out).unwrap()
}

#[cfg(target_os = "linux")]
#[test]
fn an_update_replaces_the_installation_whole_or_not_at_all() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let install = Install::at(root.path().join("app"));
    let built = root.path().join("built");
    bundle(&built, &elf("1.0.0"), "1.0.0");
    // A file of the folder that is not the bundle's own, and a folder that is not `locales`.
    std::fs::create_dir(built.join("deps")).unwrap();
    // `quill --install`: this very program, and CEF's files beside it, become the first version.
    let program = install
        .adopt(&built.join("quill"), &version("1.0.0"))
        .unwrap();
    assert_eq!(program, install.root().join("current/quill"));
    assert_eq!(std::fs::read(&program).unwrap(), elf("1.0.0"));
    let current = install.root().join("current");
    let read = |name: &str| std::fs::read_to_string(current.join(name)).unwrap();
    assert_eq!(read("libcef.so"), "cef 1.0.0");
    assert_eq!(read("locales/en-US.pak"), "en-US 1.0.0");
    assert!(!current.join("deps").exists());
    assert_eq!(install.current().as_deref(), Some("1.0.0"));
    let mode = std::fs::metadata(&program).unwrap().permissions().mode();
    assert_eq!(mode & 0o111, 0o111, "executable");
    let running = std::fs::canonicalize(&program).unwrap();
    assert!(install.runs(&running));
    assert!(!install.runs(&built.join("quill")));
    // The installed program asked again: nothing to do.
    assert_eq!(install.adopt(&running, &version("1.0.0")).unwrap(), program);

    let artifact = |name: &str, program: &[u8], marker: &str| {
        let dir = root.path().join(name);
        bundle(&dir, program, marker);
        let out = root.path().join(format!("{name}.tar.gz"));
        tar_gz(&dir, &out);
        out
    };
    // A release whose program is not a Linux program changes nothing.
    let wrong = artifact("wrong", b"MZ windows", "1.1.0");
    assert!(install.apply(&wrong, &version("1.1.0")).is_err());
    // Nor does a damaged archive.
    let good = artifact("good", &elf("1.1.0"), "1.1.0");
    let damaged = root.path().join("damaged.tar.gz");
    std::fs::write(&damaged, &std::fs::read(&good).unwrap()[..40]).unwrap();
    assert!(install.apply(&damaged, &version("1.1.0")).is_err());
    assert_eq!(install.current().as_deref(), Some("1.0.0"));
    assert_eq!(std::fs::read(&program).unwrap(), elf("1.0.0"));
    assert_eq!(read("libcef.so"), "cef 1.0.0");
    let names = |install: &Install| {
        let mut names: Vec<String> = std::fs::read_dir(install.root().join("versions"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    assert_eq!(names(&install), ["1.0.0"], "no half-written version left");

    assert_eq!(install.apply(&good, &version("1.1.0")).unwrap(), program);
    assert_eq!(std::fs::read(&program).unwrap(), elf("1.1.0"));
    assert_eq!(read("libcef.so"), "cef 1.1.0");
    assert_eq!(read("locales/en-US.pak"), "en-US 1.1.0");
    let mode = std::fs::metadata(&program).unwrap().permissions().mode();
    assert_eq!(mode & 0o111, 0o111, "the archive's modes are kept");
    // The one running before stays, for whoever still runs it.
    assert_eq!(std::fs::read(&running).unwrap(), elf("1.0.0"));
    let newest = artifact("newest", &elf("1.2.0"), "1.2.0");
    install.apply(&newest, &version("1.2.0")).unwrap();
    assert_eq!(names(&install), ["1.1.0", "1.2.0"]);
    assert_eq!(std::fs::read(&program).unwrap(), elf("1.2.0"));
    assert!(!install.root().join("current.new").exists());
    // The same version again is nothing to do.
    install.apply(&newest, &version("1.2.0")).unwrap();
    assert_eq!(install.current().as_deref(), Some("1.2.0"));
}

#[cfg(target_os = "linux")]
#[test]
fn a_program_built_alone_is_installed_alone() {
    let root = tempfile::tempdir().unwrap();
    let install = Install::at(root.path().join("app"));
    let built = root.path().join("target/release");
    std::fs::create_dir_all(&built).unwrap();
    std::fs::write(built.join("quill"), elf("1.0.0")).unwrap();
    std::fs::write(built.join("quill.d"), "not CEF's").unwrap();
    let program = install
        .adopt(&built.join("quill"), &version("1.0.0"))
        .unwrap();
    assert_eq!(std::fs::read(&program).unwrap(), elf("1.0.0"));
    let names: Vec<_> = std::fs::read_dir(install.root().join("current"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["quill"]);
}

/// A staged version changes nothing that starts; the next start activates it on trial, counts
/// its starts, and after two that never ran a minute goes back to the one before, which is
/// refused from then on. A version that runs a minute ends its trial.
#[cfg(target_os = "linux")]
#[test]
fn a_staged_version_starts_next_on_trial_and_two_failed_starts_roll_it_back() {
    use crate::install::{Launch, Staged, Trial};

    let root = tempfile::tempdir().unwrap();
    let install = Install::at(root.path().join("app"));
    let built = root.path().join("built");
    bundle(&built, &elf("1.0.0"), "1.0.0");
    let program = install
        .adopt(&built.join("quill"), &version("1.0.0"))
        .unwrap();
    let artifact = |name: &str, marker: &str| {
        let dir = root.path().join(name);
        bundle(&dir, &elf(marker), marker);
        let out = root.path().join(format!("{name}.tar.gz"));
        tar_gz(&dir, &out);
        out
    };
    // Staged: in its place, and still 1.0.0 starts.
    let staged = install
        .stage(&artifact("next", "1.1.0"), &version("1.1.0"))
        .unwrap();
    assert_eq!(staged, install.root().join("versions/1.1.0"));
    assert_eq!(std::fs::read(staged.join("quill")).unwrap(), elf("1.1.0"));
    assert_eq!(install.current().as_deref(), Some("1.0.0"));
    assert_eq!(std::fs::read(&program).unwrap(), elf("1.0.0"));
    let mark = Staged {
        version: version("1.1.0"),
        sha256: "ab".repeat(32),
        path: staged,
        since: 7,
    };
    install.keep_staged(&mark).unwrap();
    assert_eq!(install.staged(), Some(mark));
    // Nothing staged is activated by 1.1.0 itself, or by a newer copy: it goes.
    let later = Install::at(root.path().join("later"));
    later
        .keep_staged(&Staged {
            version: version("1.0.0"),
            sha256: String::new(),
            path: "x".into(),
            since: 0,
        })
        .unwrap();
    assert_eq!(later.launch(&version("1.0.0")).unwrap(), Launch::Go);
    assert_eq!(later.staged(), None);

    // The next start of 1.0.0 activates it, on trial.
    assert_eq!(
        install.launch(&version("1.0.0")).unwrap(),
        Launch::Activated {
            version: version("1.1.0"),
            program: program.clone()
        }
    );
    assert_eq!(install.current().as_deref(), Some("1.1.0"));
    assert_eq!(std::fs::read(&program).unwrap(), elf("1.1.0"));
    assert_eq!(install.staged(), None);
    let trial = |attempts| Trial {
        version: version("1.1.0"),
        previous: version("1.0.0"),
        attempts,
    };
    assert_eq!(install.trial(), Some(trial(0)));
    // Its starts are counted; one that ran a minute ends the trial.
    assert_eq!(install.launch(&version("1.1.0")).unwrap(), Launch::Go);
    assert_eq!(install.trial(), Some(trial(1)));
    install.healthy(&version("1.0.0"));
    assert_eq!(
        install.trial(),
        Some(trial(1)),
        "another version's run counts for nothing"
    );
    install.healthy(&version("1.1.0"));
    assert_eq!(install.trial(), None);

    // 1.2.0 never runs a minute: two starts, and the third goes back to 1.1.0.
    install
        .stage(&artifact("newest", "1.2.0"), &version("1.2.0"))
        .unwrap();
    let staged = install.staged_mark("1.2.0");
    install.activate_staged(&staged, &version("1.1.0")).unwrap();
    assert_eq!(install.launch(&version("1.2.0")).unwrap(), Launch::Go);
    assert_eq!(install.launch(&version("1.2.0")).unwrap(), Launch::Go);
    assert_eq!(install.bad(), None);
    assert_eq!(
        install.launch(&version("1.2.0")).unwrap(),
        Launch::RolledBack {
            version: version("1.2.0"),
            previous: version("1.1.0"),
            program: program.clone()
        }
    );
    assert_eq!(install.current().as_deref(), Some("1.1.0"));
    assert_eq!(std::fs::read(&program).unwrap(), elf("1.1.0"));
    assert_eq!(install.trial(), None);
    assert_eq!(install.bad(), Some(version("1.2.0")));
    // 1.1.0 starts as itself from then on.
    assert_eq!(install.launch(&version("1.1.0")).unwrap(), Launch::Go);
    // Nothing staged cannot be activated, and leaves no trial.
    assert!(install
        .activate_staged(&install.staged_mark("9.0.0"), &version("1.1.0"))
        .is_err());
    assert_eq!(install.trial(), None);
    assert_eq!(install.current().as_deref(), Some("1.1.0"));
}

/// 0.1.9 installs a release of Newen, quill's next name: its manifest under Newen's name, its
/// program `newen`. The version staged gets a `quill` link to it, so launchers of
/// `current/quill` start Newen; the next start activates it on trial and starts
/// `current/newen`. When Newen's starts never run a minute (counted by this same code, which
/// Newen keeps), its third goes back to 0.1.9, started as `current/quill` again. File by file,
/// the same layout.
#[cfg(target_os = "linux")]
#[test]
fn a_release_of_newen_is_staged_started_on_trial_and_rolled_back_to_quill() {
    use crate::install::Launch;
    use std::path::Path;

    let signer = key();
    let root = tempfile::tempdir().unwrap();
    let install = Install::at(root.path().join("app"));
    let built = root.path().join("built");
    bundle(&built, &elf("0.1.9"), "0.1.9");
    let quill = install
        .adopt(&built.join("quill"), &version("0.1.9"))
        .unwrap();
    // Newen's bundle: `newen` and CEF's files, and no `quill`.
    let tree = root.path().join("newen");
    bundle(&tree, &elf("0.2.0"), "0.2.0");
    std::fs::rename(tree.join("quill"), tree.join("newen")).unwrap();
    let archive = tar_gz_of(
        &tree,
        &root.path().join("newen-x86_64-linux.tar.gz"),
        &["newen", "libcef.so", "locales"],
    );
    let server = serve(0).unwrap();
    server.put("/newen-x86_64-linux.tar.gz", archive.clone());
    let url = format!("{}/newen-x86_64-linux.tar.gz", server.base());
    let newen = Manifest {
        product: NEWEN_PRODUCT.into(),
        ..manifest("0.2.0", None, &[(&platform(), artifact(&url, &archive))])
    };
    publish_as(&server, &signer, &newen, NEWEN_MANIFEST);
    let agent = agent();
    let trust = Trust::new(vec![signer.verifying_key()]);
    let found = check(&agent, &Source::at(server.base()), &trust).unwrap();
    let offered = offer(found, &version("0.1.9"), &platform()).unwrap();
    let file = download(
        &agent,
        offered.artifact.as_ref().unwrap(),
        &install.downloads(),
        |_, _| true,
    )
    .unwrap();
    let staged = install.stage(&file, &offered.version).unwrap();
    assert_eq!(std::fs::read(staged.join("newen")).unwrap(), elf("0.2.0"));
    assert_eq!(
        std::fs::read_link(staged.join("quill")).unwrap(),
        Path::new("newen")
    );
    assert_eq!(install.program(), quill, "0.1.9 still starts");
    install.keep_staged(&install.staged_mark("0.2.0")).unwrap();

    // The next start of 0.1.9 activates Newen, on trial, and starts its program.
    let started = install.root().join("current/newen");
    assert_eq!(
        install.launch(&version("0.1.9")).unwrap(),
        Launch::Activated {
            version: version("0.2.0"),
            program: started.clone()
        }
    );
    assert_eq!(install.program(), started);
    assert_eq!(std::fs::read(&quill).unwrap(), elf("0.2.0"), "a launcher's");
    // Newen never runs a minute: its third start goes back to 0.1.9, refused from then on.
    assert_eq!(install.launch(&version("0.2.0")).unwrap(), Launch::Go);
    assert_eq!(install.launch(&version("0.2.0")).unwrap(), Launch::Go);
    assert_eq!(
        install.launch(&version("0.2.0")).unwrap(),
        Launch::RolledBack {
            version: version("0.2.0"),
            previous: version("0.1.9"),
            program: quill.clone()
        }
    );
    assert_eq!(install.current().as_deref(), Some("0.1.9"));
    assert_eq!(install.program(), quill);
    assert_eq!(std::fs::read(&quill).unwrap(), elf("0.1.9"));
    assert_eq!(install.bad(), Some(version("0.2.0")));

    // File by file, from its list and blobs: the same layout.
    let later = Install::at(root.path().join("later"));
    later
        .adopt(&built.join("quill"), &version("0.1.9"))
        .unwrap();
    let entry = serve::publish_files(&server, &tree, &version("0.2.0"), &platform());
    let list = files::fetch_bundle(&agent, &entry, &version("0.2.0"), &platform()).unwrap();
    let blobs = later.downloads().join("blobs");
    let staged = files::stage(&agent, &later, &list, &blobs, |_, _| true).unwrap();
    assert_eq!(std::fs::read(staged.join("newen")).unwrap(), elf("0.2.0"));
    assert_eq!(
        std::fs::read_link(staged.join("quill")).unwrap(),
        Path::new("newen")
    );
    assert_eq!(
        later.activate(&version("0.2.0")).unwrap(),
        later.root().join("current/newen")
    );
}

/// On Linux the installation is `quill/app` in the data folder, and `newen/app` once Newen
/// moved it there, with or without `quill/` left as a link to it.
#[test]
fn the_installation_is_found_where_newen_moved_it() {
    use crate::install::app_root;
    let data = tempfile::tempdir().unwrap();
    let data = data.path();
    assert_eq!(app_root(data), data.join("quill/app"));
    std::fs::create_dir_all(data.join("quill/app/versions")).unwrap();
    assert_eq!(app_root(data), data.join("quill/app"));
    // Newen's folder without an installation in it is not one.
    std::fs::create_dir_all(data.join("newen")).unwrap();
    assert_eq!(app_root(data), data.join("quill/app"));
    std::fs::remove_dir(data.join("newen")).unwrap();
    std::fs::rename(data.join("quill"), data.join("newen")).unwrap();
    assert_eq!(app_root(data), data.join("newen/app"));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("newen", data.join("quill")).unwrap();
        assert_eq!(app_root(data), data.join("newen/app"));
    }
}

/// A bundle's program is Newen's when it has one, else quill's, on every system.
#[test]
fn a_bundle_s_program_is_newen_s_when_it_has_one() {
    use crate::install::{program_in, NEWEN_PROGRAM, PROGRAM};
    let folder = tempfile::tempdir().unwrap();
    assert_eq!(program_in(folder.path()), folder.path().join(PROGRAM));
    std::fs::write(folder.path().join(PROGRAM), "quill").unwrap();
    assert_eq!(program_in(folder.path()), folder.path().join(PROGRAM));
    std::fs::write(folder.path().join(NEWEN_PROGRAM), "newen").unwrap();
    assert_eq!(program_in(folder.path()), folder.path().join(NEWEN_PROGRAM));
}

#[cfg(target_os = "linux")]
impl Install {
    fn staged_mark(&self, at: &str) -> crate::install::Staged {
        crate::install::Staged {
            version: version(at),
            sha256: String::new(),
            path: self.root().join("versions").join(at),
            since: 0,
        }
    }
}

/// What a start decides, on every system: the trial first, then a newer staged version.
#[test]
fn a_start_counts_its_trial_rolls_back_after_two_and_else_activates_what_is_newer() {
    use crate::install::{staged_path, step, Staged, Step, Trial};
    use std::path::Path;

    let trial = |attempts| Trial {
        version: version("1.1.0"),
        previous: version("1.0.0"),
        attempts,
    };
    let staged = |at: &str| Staged {
        version: version(at),
        sha256: String::new(),
        path: "x".into(),
        since: 0,
    };
    let running = version("1.1.0");
    assert_eq!(step(&running, None, None), Step::Go);
    assert_eq!(step(&running, Some(&trial(0)), None), Step::Count(trial(1)));
    assert_eq!(step(&running, Some(&trial(1)), None), Step::Count(trial(2)));
    assert_eq!(
        step(&running, Some(&trial(2)), None),
        Step::RollBack(trial(2))
    );
    // The trial comes before anything staged.
    assert_eq!(
        step(&running, Some(&trial(0)), Some(&staged("2.0.0"))),
        Step::Count(trial(1))
    );
    // Another version's trial is not this start's.
    assert_eq!(
        step(&version("1.0.0"), Some(&trial(2)), Some(&staged("1.1.0"))),
        Step::Activate(staged("1.1.0"))
    );
    assert_eq!(step(&running, None, Some(&staged("1.1.0"))), Step::Forget);
    assert_eq!(step(&running, None, Some(&staged("1.0.9"))), Step::Forget);

    let root = Path::new("/apps");
    let at = version("0.1.9");
    assert_eq!(
        staged_path("linux", root, &at),
        Path::new("/apps/versions/0.1.9")
    );
    assert_eq!(
        staged_path("windows", root, &at),
        Path::new("/apps/quill.exe.0.1.9.new")
    );
    assert_eq!(
        staged_path("macos", root, &at),
        Path::new("/apps/.quill.app.0.1.9")
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_release_goes_from_a_loopback_feed_to_the_installation() {
    let signer = key();
    let folder = tempfile::tempdir().unwrap();
    bundle(&folder.path().join("bundle"), &elf("2.0.0"), "2.0.0");
    let built = tar_gz(
        &folder.path().join("bundle"),
        &folder.path().join("quill-x86_64-linux.tar.gz"),
    );
    let server = serve(0).unwrap();
    server.put("/quill-x86_64-linux.tar.gz", built.clone());
    let linux = artifact(
        &format!("{}/quill-x86_64-linux.tar.gz", server.base()),
        &built,
    );
    publish(
        &server,
        &signer,
        &manifest("2.0.0", Some("2.0.0"), &[(&platform(), linux)]),
    );

    let agent = agent();
    let source = Source::at(server.base());
    let manifest = check(&agent, &source, &Trust::new(vec![signer.verifying_key()])).unwrap();
    let offered = offer(manifest, &version("1.0.0"), &platform()).unwrap();
    assert!(offered.mandatory);
    let root = tempfile::tempdir().unwrap();
    let install = Install::at(root.path());
    let file = download(
        &agent,
        offered.artifact.as_ref().unwrap(),
        &install.downloads(),
        |_, _| true,
    )
    .unwrap();
    let program = install.apply(&file, &offered.version).unwrap();
    assert_eq!(std::fs::read(&program).unwrap(), elf("2.0.0"));
    assert_eq!(
        std::fs::read_to_string(program.with_file_name("libcef.so")).unwrap(),
        "cef 2.0.0"
    );
}

/// A manifest with `whats_new` and one without both read, and the field is left out when
/// there is none, so an old release's bytes stay as they were. A copy from before the field
/// reads a manifest that has it, as serde skips any field it does not know.
#[test]
fn a_manifest_reads_with_or_without_whats_new_and_older_copies_ignore_it() {
    let signer = key();
    let plain = manifest("1.0.0", None, &[]);
    let bytes = serde_json::to_vec(&plain).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("whats_new"));
    assert_eq!(
        verify(&bytes, &sign(&signer, &bytes), &signer.verifying_key()),
        Ok(plain.clone())
    );
    let entry = artifact("https://example.org/whats-new.json", b"{}");
    let with = Manifest {
        whats_new: Some(entry.clone()),
        ..plain.clone()
    };
    let bytes = serde_json::to_vec(&with).unwrap();
    let read = verify(&bytes, &sign(&signer, &bytes), &signer.verifying_key()).unwrap();
    assert_eq!(read.whats_new, Some(entry.clone()));
    // What a copy without the field does: a field it does not know is skipped.
    let mut later: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    later["something_later"] = serde_json::json!({ "url": "https://example.org/x" });
    let later: Manifest = serde_json::from_value(later).unwrap();
    assert_eq!(later.whats_new, Some(entry.clone()));
    // Offered with the release; one that could not be downloaded is not.
    let offered = offer(with.clone(), &version("0.9.0"), "linux-x86_64").unwrap();
    assert_eq!(offered.whats_new, Some(entry));
    let plain_http = Manifest {
        whats_new: Some(artifact("http://example.org/whats-new.json", b"{}")),
        ..plain
    };
    let offered = offer(plain_http, &version("0.9.0"), "linux-x86_64").unwrap();
    assert_eq!(offered.whats_new, None);
}

/// A release on loopback whose what's new names `media`, each served as given: the release
/// offered, its source and the server.
fn whats_new_release(media: &[(whats_new::Kind, &[u8], &[u8])]) -> (Release, Server) {
    let server = serve(0).unwrap();
    let medium = |name: &str, bytes: &[u8], served: &[u8]| {
        server.put(&format!("/{name}"), served.to_vec());
        artifact(&format!("{}/{name}", server.base()), bytes)
    };
    let media = media
        .iter()
        .enumerate()
        .map(|(at, (kind, bytes, served))| whats_new::Medium {
            kind: *kind,
            file: medium(&format!("medium-{at}.png"), bytes, served),
            poster: (*kind == whats_new::Kind::Video)
                .then(|| medium(&format!("poster-{at}.png"), b"poster", b"poster")),
        })
        .collect();
    let notes = whats_new::WhatsNew {
        version: version("2.0.0"),
        date: "2026-11-10".into(),
        notes: whats_new::Notes {
            new: vec!["A browser beside the chat".into()],
            ..Default::default()
        },
        announcement: Some(whats_new::Announcement {
            title: "A browser beside the chat".into(),
            body: "Open any page next to the session.".into(),
            bullets: vec![],
            media,
            try_it: Some("browser".into()),
        }),
    };
    let json = serde_json::to_vec(&notes).unwrap();
    let entry = medium(whats_new::FILE, &json, &json);
    let release = Release {
        version: version("2.0.0"),
        date: String::new(),
        notes: String::new(),
        mandatory: false,
        artifact: None,
        whats_new: Some(entry),
        files: None,
    };
    (release, server)
}

/// Each medium is kept only when it is the size and digest the signed notes name: one that is
/// not is refused and the next is tried; the words come through either way.
#[test]
fn a_medium_that_does_not_match_its_digest_is_refused_and_the_words_still_come() {
    use whats_new::{fetch_whats_new, Kind};
    let agent = agent();
    let folder = tempfile::tempdir().unwrap();
    // The first picture was changed on the server; the clip after it matches.
    let (release, _server) = whats_new_release(&[
        (Kind::Image, b"the picture", b"another picture"),
        (Kind::Video, b"the clip", b"the clip"),
    ]);
    let (notes, kept) = fetch_whats_new(&agent, &release, folder.path()).unwrap();
    assert_eq!(notes.notes.new, ["A browser beside the chat"]);
    let kept = kept.expect("the clip");
    assert_eq!(kept.kind, Kind::Video);
    assert_eq!(std::fs::read(&kept.file).unwrap(), b"the clip");
    assert_eq!(std::fs::read(kept.poster.unwrap()).unwrap(), b"poster");
    // Only what matched is on the disk.
    let names: Vec<_> = std::fs::read_dir(folder.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    // None matches: no medium, the words still.
    let folder = tempfile::tempdir().unwrap();
    let (release, _server) =
        whats_new_release(&[(Kind::Image, b"the picture", b"the picture, longer")]);
    let (notes, kept) = fetch_whats_new(&agent, &release, folder.path()).unwrap();
    assert_eq!((notes.version, kept), (version("2.0.0"), None));
    // The notes themselves changed after signing: refused whole.
    let (release, server) = whats_new_release(&[]);
    server.put(
        &format!("/{}", whats_new::FILE),
        b"{\"version\":\"2.0.0\"}".to_vec(),
    );
    assert_eq!(
        fetch_whats_new(&agent, &release, folder.path()),
        Err(Error::Digest)
    );
    // Another release's notes, signed as this one's.
    let (release, _server) = whats_new_release(&[]);
    let release = Release {
        version: version("3.0.0"),
        ..release
    };
    assert_eq!(
        fetch_whats_new(&agent, &release, folder.path()),
        Err(Error::Manifest)
    );
}

/// A manifest with every field a copy up to 0.1.8 does not know is read by that copy's own
/// structs (no unknown field refused), and stays far below its 64 KiB limit: the lists of files
/// are named, not inlined.
#[test]
fn a_v2_manifest_is_read_by_a_copy_from_before_it() {
    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct OldArtifact {
        url: String,
        size: u64,
        sha256: String,
    }
    /// `Manifest` as 0.1.8 has it.
    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct Old {
        version: Version,
        date: String,
        #[serde(default)]
        notes: String,
        #[serde(default)]
        minimum_version: Option<Version>,
        platforms: BTreeMap<String, OldArtifact>,
        #[serde(default)]
        whats_new: Option<OldArtifact>,
    }
    let archive = artifact("https://example.org/quill-x86_64-linux.tar.gz", b"quill");
    let list = artifact("https://example.org/files-linux-x86_64.json", b"{}");
    let platforms = ["linux-x86_64", "windows-x86_64", "macos-aarch64"];
    let manifest = Manifest {
        sequence: 12,
        expires: Some("2030-01-01T00:00:00Z".into()),
        next_keys: vec![write_keys(&[key().verifying_key()])],
        files: platforms
            .iter()
            .map(|p| (p.to_string(), list.clone()))
            .collect(),
        ..manifest(
            "1.2.0",
            Some("1.1.0"),
            &platforms.map(|p| (p, archive.clone())),
        )
    };
    let bytes = serde_json::to_vec_pretty(&manifest).unwrap();
    assert!(bytes.len() < 8 * 1024, "{}", bytes.len());
    let old: Old = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(old.version, version("1.2.0"));
    assert_eq!(old.platforms["linux-x86_64"].url, archive.url);
    // And this copy reads back what it wrote.
    let read: Manifest = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(read, manifest);
}

/// The backup key is compiled in beside the release key, and a manifest signed by either key
/// trusted verifies; one signed by neither does not.
#[test]
fn a_manifest_signed_by_the_backup_key_is_accepted() {
    let compiled = compiled_keys();
    assert_eq!(compiled.len(), 2, "release-key.pub and backup-key.pub");
    assert_eq!(compiled[1], parse_key(BACKUP_KEY).unwrap());
    assert_eq!(trusted_keys(&[]), compiled);
    let (release, backup) = (key(), key());
    let trusted = [release.verifying_key(), backup.verifying_key()];
    let bytes = serde_json::to_vec(&manifest("1.2.0", None, &[])).unwrap();
    let by_backup = sign(&backup, &bytes);
    assert!(verify_signed(&bytes, &[&by_backup], &trusted).is_ok());
    // The backup's signature among others that verify with nothing.
    let other = sign(&key(), &bytes);
    assert!(verify_signed(&bytes, &[&other, "garbage", &by_backup], &trusted).is_ok());
    assert_eq!(
        verify_signed(&bytes, &[&other], &trusted),
        Err(Error::Signature)
    );
}

/// A rotation (`next_keys`) is accepted only when a key trusted now and every new key signed
/// the manifest; the new set is then what is trusted, and the old key alone is refused.
#[test]
fn a_rotation_needs_the_old_and_the_new_keys_signatures() {
    let (old, new, backup) = (key(), key(), key());
    let trusted = [old.verifying_key(), backup.verifying_key()];
    let next = [new.verifying_key(), backup.verifying_key()];
    let rotating = Manifest {
        next_keys: next.iter().map(|k| write_keys(&[*k])).collect(),
        ..manifest("1.2.0", None, &[])
    };
    let bytes = serde_json::to_vec(&rotating).unwrap();
    let (by_old, by_new) = (sign(&old, &bytes), sign(&new, &bytes));
    // The old key alone: the new one never proved itself.
    assert_eq!(
        verify_signed(&bytes, &[&by_old], &trusted),
        Err(Error::Signature)
    );
    // The new key alone: nothing trusted signed it.
    assert_eq!(
        verify_signed(&bytes, &[&by_new], &trusted),
        Err(Error::Signature)
    );
    let read = verify_signed(&bytes, &[&by_old, &by_new], &trusted).unwrap();
    assert_eq!(read.rotated, Some(next.to_vec()));
    // Kept as text and read back: the keys trusted from now on.
    let kept = read_keys(&write_keys(&next)).unwrap();
    assert_eq!(trusted_keys(&kept), next);
    // The same manifest seen again by a copy that rotated: nothing new.
    assert_eq!(
        verify_signed(&bytes, &[&by_old, &by_new], &next)
            .unwrap()
            .rotated,
        None
    );
    // After it, the old key no longer signs anything this copy trusts.
    let later = serde_json::to_vec(&manifest("1.3.0", None, &[])).unwrap();
    assert_eq!(
        verify_signed(&later, &[&sign(&old, &later)], &next),
        Err(Error::Signature)
    );
    assert!(verify_signed(&later, &[&sign(&new, &later)], &next).is_ok());
    assert_eq!(read_keys("not-a-key"), None);
}

/// A manifest lower than the highest sequence seen, of another product or channel, from before
/// those fields, or expired, is refused; one at the same sequence is not.
#[test]
fn a_lower_sequence_another_product_or_an_expired_manifest_is_refused() {
    let now = 1_793_631_600; // 2026-11-02T15:00:00Z
    let at = |sequence| Manifest {
        sequence,
        ..manifest("1.2.0", None, &[])
    };
    assert_eq!(admit(&at(5), 5, now), Ok(()));
    assert_eq!(admit(&at(6), 5, now), Ok(()));
    assert_eq!(admit(&at(4), 5, now), Err(Error::Replayed));
    let foreign = |product: &str, channel: &str| Manifest {
        product: product.into(),
        channel: channel.into(),
        ..at(5)
    };
    assert_eq!(
        admit(&foreign("other", CHANNEL), 0, now),
        Err(Error::Foreign)
    );
    assert_eq!(
        admit(&foreign(PRODUCT, "beta"), 0, now),
        Err(Error::Foreign)
    );
    // A manifest from before the fields.
    let old: Manifest =
        serde_json::from_str(r#"{"version":"1.2.0","date":"","platforms":{}}"#).unwrap();
    assert_eq!(admit(&old, 0, now), Err(Error::Foreign));
    let expiring = |expires: &str| Manifest {
        expires: Some(expires.into()),
        ..at(5)
    };
    assert_eq!(admit(&expiring("2026-11-02T15:00:01Z"), 0, now), Ok(()));
    assert_eq!(
        admit(&expiring("2026-11-02T12:00:00-03:00"), 0, now),
        Err(Error::Expired)
    );
    assert_eq!(admit(&expiring("tomorrow"), 0, now), Err(Error::Expired));

    // Through a check: the copy's highest sequence is part of what it trusts.
    let signer = key();
    let server = serve(0).unwrap();
    publish(&server, &signer, &at(4));
    let feed = Source::at(server.base());
    let trust = |sequence| Trust {
        keys: vec![signer.verifying_key()],
        sequence,
    };
    assert_eq!(check(&agent(), &feed, &trust(5)), Err(Error::Replayed));
    assert_eq!(check(&agent(), &feed, &trust(4)).unwrap().sequence, 4);
    publish(&server, &signer, &expiring("2020-01-01T00:00:00Z"));
    assert_eq!(check(&agent(), &feed, &trust(0)), Err(Error::Expired));
}

#[test]
fn rfc3339_dates_are_read_as_seconds_since_1970() {
    assert_eq!(rfc3339("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(rfc3339("2026-11-02T12:00:00-03:00"), Some(1_793_631_600));
    assert_eq!(rfc3339("2026-11-02T15:00:00.250Z"), Some(1_793_631_600));
    assert_eq!(rfc3339("2026-11-02T16:30:00+01:30"), Some(1_793_631_600));
    assert_eq!(rfc3339("2000-02-29T00:00:00Z"), Some(951_782_400));
    for wrong in [
        "",
        "2026-11-02",
        "2026-13-02T12:00:00Z",
        "2026-11-02T12:00:00",
        "2026-11-02 12:00:00Z",
        "+026-11-02T12:00:00Z",
    ] {
        assert_eq!(rfc3339(wrong), None, "{wrong}");
    }
}

/// A list whose paths leave the bundle, repeat, or whose links point out of it, is refused.
#[test]
fn a_list_of_files_with_a_path_out_of_the_bundle_is_refused() {
    use files::{valid, Bundle, File, Link};
    let file = |path: &str| File {
        path: path.into(),
        size: 1,
        sha256: "a".repeat(64),
        mode: 0o644,
        url: "https://example.org/f.gz".into(),
        packed: 1,
    };
    let bundle = |files: Vec<File>, links: Vec<(&str, &str)>| Bundle {
        version: version("1.0.0"),
        platform: "linux-x86_64".into(),
        files,
        links: (links.into_iter())
            .map(|(path, target)| Link {
                path: path.into(),
                target: target.into(),
            })
            .collect(),
    };
    let fine = bundle(
        vec![file("quill"), file("quill.app/Contents/MacOS/quill")],
        vec![("a/b/Current", "../x"), ("lib", "quill")],
    );
    assert!(valid(&fine));
    for path in [
        "../quill",
        "/etc/passwd",
        "a//b",
        "a/./b",
        "C:x",
        "a\\b",
        "",
    ] {
        assert!(!valid(&bundle(vec![file(path)], vec![])), "{path}");
    }
    assert!(!valid(&bundle(vec![file("quill"), file("quill")], vec![])));
    for target in ["../x", "/abs", "a/../../x"] {
        assert!(!valid(&bundle(vec![], vec![("link", target)])), "{target}");
    }
    let mut http = file("quill");
    http.url = "http://example.org/f.gz".into();
    assert!(!valid(&bundle(vec![http], vec![])));
}

/// Every entry under `dir`: a file's bytes and mode, or a link's target.
#[cfg(target_os = "linux")]
fn tree(dir: &std::path::Path) -> BTreeMap<String, (Vec<u8>, u32)> {
    use std::os::unix::fs::PermissionsExt;
    let mut found = BTreeMap::new();
    let mut pending = vec![(dir.to_path_buf(), String::new())];
    while let Some((folder, prefix)) = pending.pop() {
        for entry in std::fs::read_dir(&folder).unwrap().flatten() {
            let path = format!("{prefix}{}", entry.file_name().to_string_lossy());
            let kind = entry.file_type().unwrap();
            if kind.is_symlink() {
                let target = std::fs::read_link(entry.path()).unwrap();
                found.insert(path, (target.to_string_lossy().as_bytes().to_vec(), 0));
            } else if kind.is_dir() {
                pending.push((entry.path(), format!("{path}/")));
            } else {
                let mode = entry.metadata().unwrap().permissions().mode() & 0o777;
                found.insert(path, (std::fs::read(entry.path()).unwrap(), mode));
            }
        }
    }
    found
}

/// Version 1.0.0 installed from one bundle, and the bundle of 2.0.0 in another folder: its
/// program changed, CEF's files did not, a file with its own mode and two links are new.
#[cfg(target_os = "linux")]
fn installed_and_next(folder: &std::path::Path) -> (Install, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let one = folder.join("one");
    bundle(&one, &elf("1.0.0"), "same");
    let install = Install::at(folder.join("app"));
    install
        .adopt(&one.join("quill"), &version("1.0.0"))
        .unwrap();
    let two = folder.join("two");
    bundle(&two, &elf("2.0.0"), "same");
    std::fs::create_dir_all(two.join("extra")).unwrap();
    std::fs::write(two.join("extra/data.bin"), program(70_000)).unwrap();
    std::fs::set_permissions(
        two.join("extra/data.bin"),
        std::fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    std::os::unix::fs::symlink("libcef.so", two.join("libcef-link.so")).unwrap();
    std::os::unix::fs::symlink("../locales", two.join("extra/locales")).unwrap();
    (install, two)
}

/// The next version is staged file by file: the files the running version has are taken from
/// it (a hard link when the mode matches), only the changed ones are downloaded, and the folder
/// staged is exactly the release's bundle, modes and links included.
#[cfg(target_os = "linux")]
#[test]
fn a_bundle_is_staged_from_local_files_and_only_the_changed_blobs() {
    use std::os::unix::fs::MetadataExt;
    let folder = tempfile::tempdir().unwrap();
    let (install, two) = installed_and_next(folder.path());
    let server = serve(0).unwrap();
    let entry = serve::publish_files(&server, &two, &version("2.0.0"), "linux-x86_64");
    let agent = agent();
    let bundle = files::fetch_bundle(&agent, &entry, &version("2.0.0"), "linux-x86_64").unwrap();
    // Another version's list, or one for another system, is refused.
    assert_eq!(
        files::fetch_bundle(&agent, &entry, &version("2.0.1"), "linux-x86_64"),
        Err(Error::Manifest)
    );
    let blobs = install.downloads().join("blobs");
    let mut told = Vec::new();
    let staged = files::stage(&agent, &install, &bundle, &blobs, |done, total| {
        told.push((done, total));
        true
    })
    .unwrap();
    assert_eq!(staged, install.root().join("versions/2.0.0"));
    assert_eq!(tree(&staged), tree(&two));
    // The program and the new file came; CEF's files and the locale did not.
    let fetched: Vec<String> = (server.requests().into_iter())
        .map(|(path, _)| path)
        .filter(|path| path.starts_with("/f-"))
        .collect();
    let blob_of = |path: &str| {
        let file = bundle.files.iter().find(|file| file.path == path).unwrap();
        (format!("/{}", files::blob_name(&file.sha256)), file.packed)
    };
    let (quill, data) = (blob_of("quill"), blob_of("extra/data.bin"));
    let mut expected = vec![quill.0.clone(), data.0.clone()];
    expected.sort();
    let mut fetched_sorted = fetched.clone();
    fetched_sorted.sort();
    assert_eq!(fetched_sorted, expected);
    // Progress counts the bytes downloaded, not the bundle.
    assert_eq!(told.last(), Some(&(quill.1 + data.1, quill.1 + data.1)));
    let inode = |path: std::path::PathBuf| std::fs::metadata(path).unwrap().ino();
    assert_eq!(
        inode(install.root().join("current/libcef.so")),
        inode(staged.join("libcef.so"))
    );
    assert!(!blobs.exists(), "the blobs go once staged");
    // Still staged only: 1.0.0 starts.
    assert_eq!(install.current().as_deref(), Some("1.0.0"));
}

/// A blob that is not its file stops the staging: nothing is staged, the blob goes, and the
/// caller falls back on the full archive.
#[cfg(target_os = "linux")]
#[test]
fn a_bad_blob_stages_nothing() {
    let folder = tempfile::tempdir().unwrap();
    let (install, two) = installed_and_next(folder.path());
    let server = serve(0).unwrap();
    let entry = serve::publish_files(&server, &two, &version("2.0.0"), "linux-x86_64");
    let agent = agent();
    let bundle = files::fetch_bundle(&agent, &entry, &version("2.0.0"), "linux-x86_64").unwrap();
    let quill = bundle
        .files
        .iter()
        .find(|file| file.path == "quill")
        .unwrap();
    let name = files::blob_name(&quill.sha256);
    // The same size, other bytes: a valid gzip of something else would do the same.
    server.put(&format!("/{name}"), vec![0x1f; quill.packed as usize]);
    let blobs = install.downloads().join("blobs");
    assert_eq!(
        files::stage(&agent, &install, &bundle, &blobs, |_, _| true),
        Err(Error::Disk)
    );
    assert!(!install.root().join("versions/2.0.0").exists());
    assert!(!blobs.join(&name).exists());
    // A blob larger than listed is cut off and refused before it is unpacked.
    server.put(&format!("/{name}"), vec![0; quill.packed as usize + 10]);
    assert_eq!(
        files::stage(&agent, &install, &bundle, &blobs, |_, _| true),
        Err(Error::Digest)
    );
}
