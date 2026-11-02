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
        whats_new: None,
        assets: Default::default(),
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
    let bytes = serde_json::to_vec_pretty(manifest).unwrap();
    server.put(&format!("/{SIGNATURE}"), sign(signer, &bytes).into_bytes());
    server.put(&format!("/{MANIFEST}"), bytes);
}

#[test]
fn a_check_reads_a_signed_feed_on_loopback_and_refuses_a_forged_one() {
    let signer = key();
    let release = manifest("9.0.0", Some("9.0.0"), &[]);
    let server = serve(0).unwrap();
    let agent = agent();
    let feed = Source::Files {
        base: server.base(),
    };
    let public = signer.verifying_key();
    // Nothing there yet.
    assert_eq!(check(&agent, &feed, Some(&public)), Err(Error::Status(404)));
    publish(&server, &signer, &release);
    assert_eq!(check(&agent, &feed, Some(&public)), Ok(release.clone()));
    assert_eq!(check(&agent, &feed, None), Err(Error::NoKey));
    // The same feed, verified with another key.
    assert_eq!(
        check(&agent, &feed, Some(&key().verifying_key())),
        Err(Error::Signature)
    );
    // A manifest changed after signing.
    server.put(
        &format!("/{MANIFEST}"),
        serde_json::to_vec_pretty(&manifest("9.0.1", None, &[])).unwrap(),
    );
    assert_eq!(check(&agent, &feed, Some(&public)), Err(Error::Signature));
    // A manifest larger than any real one.
    server.put(&format!("/{MANIFEST}"), vec![b' '; MAX_MANIFEST + 1]);
    assert_eq!(check(&agent, &feed, Some(&public)), Err(Error::TooLarge));
}

/// The private repository's API on loopback: its latest release lists `assets` by name, and
/// refuses every request for a token other than `token`.
fn api_release(server: &Server, assets: &[(&str, Vec<u8>)]) -> String {
    let listed: Vec<String> = assets
        .iter()
        .enumerate()
        .map(|(at, (name, bytes))| {
            server.put(&format!("/assets/{at}"), bytes.clone());
            format!(
                r#"{{"name":"{name}","url":"{}/assets/{at}"}}"#,
                server.base()
            )
        })
        .collect();
    server.put(
        &format!("/repos/{REPOSITORY}/releases/latest"),
        format!(r#"{{"assets":[{}]}}"#, listed.join(",")).into_bytes(),
    );
    server.base()
}

#[test]
fn a_private_release_is_read_through_the_api_and_its_artifact_is_fetched_from_there() {
    let signer = key();
    let built = program(50_000);
    let server = serve(0).unwrap();
    let linux = artifact(
        "https://github.com/Niiihuel/quill/releases/download/v2.0.0/quill-x86_64-linux.tar.gz",
        &built,
    );
    let body =
        serde_json::to_vec_pretty(&manifest("2.0.0", None, &[("linux-x86_64", linux.clone())]))
            .unwrap();
    let signature = sign(&signer, &body);
    let api = api_release(
        &server,
        &[
            (MANIFEST, body.clone()),
            (SIGNATURE, format!("{signature}\n").into_bytes()),
            ("quill-x86_64-linux.tar.gz", built.clone()),
        ],
    );
    let agent = agent();
    let source = Source::Api {
        token: "a-token".into(),
        api,
    };
    let checked = check(&agent, &source, Some(&signer.verifying_key())).unwrap();
    // The manifest's address names the release on GitHub; the API reaches the same file.
    let artifact = &checked.platforms["linux-x86_64"];
    assert_eq!(artifact.url, format!("{}/assets/2", server.base()));
    let folder = tempfile::tempdir().unwrap();
    let path = download(&agent, &source, artifact, folder.path(), |_, _| true).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), built);
    // A repository that refuses the token, or has no release at all, is about access.
    let nothing = serve(0).unwrap();
    let refused = Source::Api {
        token: "a-token".into(),
        api: nothing.base(),
    };
    assert_eq!(
        check(&agent, &refused, Some(&signer.verifying_key())),
        Err(Error::Access)
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

/// Downloads from a feed need no token.
fn files() -> Source {
    Source::Files {
        base: String::new(),
    }
}

#[test]
fn a_download_is_kept_only_when_its_size_and_digest_match() {
    let body = program(300_000);
    let (server, good) = artifact_server(&body);
    let agent = agent();
    let folder = tempfile::tempdir().unwrap();
    let mut seen = Vec::new();
    let path = download(&agent, &files(), &good, folder.path(), |have, whole| {
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
        download(&agent, &files(), &good, folder.path(), |_, _| true),
        Ok(path.clone())
    );
    assert_eq!(server.requests().len(), before);

    // Another digest: what came is deleted.
    let mut wrong = good.clone();
    wrong.sha256 = hex(&Sha256::digest(b"otra cosa"));
    assert_eq!(
        download(&agent, &files(), &wrong, folder.path(), |_, _| true),
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
        download(&agent, &files(), &short, folder.path(), |_, _| true),
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
        download(&agent, &files(), &long, folder.path(), |_, _| true),
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
    let first = download(&agent, &files(), &wanted, folder.path(), |_, _| true);
    assert!(
        matches!(first, Err(Error::Network | Error::Incomplete)),
        "{first:?}"
    );
    let part = folder.path().join(format!("quill-{}.part", wanted.sha256));
    // What arrived before the cut, as far as the client read it.
    let kept = std::fs::metadata(&part).unwrap().len();
    assert!(kept > 0 && kept <= 150_000, "{kept}");
    let path = download(&agent, &files(), &wanted, folder.path(), |_, _| true).unwrap();
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
    let stopped = download(&agent, &files(), &wanted, folder.path(), |have, _| {
        have < 100_000
    });
    assert_eq!(stopped, Err(Error::Interrupted));
    let part = folder.path().join(format!("quill-{}.part", wanted.sha256));
    assert!(std::fs::metadata(&part).unwrap().len() >= 100_000);
    let path = download(&agent, &files(), &wanted, folder.path(), |_, _| true).unwrap();
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
    assert!(download(&agent, &files(), &wanted, folder.path(), |_, _| true).is_err());
    let path = download(&agent, &files(), &wanted, folder.path(), |_, _| true).unwrap();
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
    let status = std::process::Command::new("tar")
        .arg("-czf")
        .arg(out)
        .arg("-C")
        .arg(dir)
        .args(["quill", "libcef.so", "locales"])
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
    let source = Source::Files {
        base: server.base(),
    };
    let manifest = check(&agent, &source, Some(&signer.verifying_key())).unwrap();
    let offered = offer(manifest, &version("1.0.0"), &platform()).unwrap();
    assert!(offered.mandatory);
    let root = tempfile::tempdir().unwrap();
    let install = Install::at(root.path());
    let file = download(
        &agent,
        &source,
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
fn whats_new_release(media: &[(whats_new::Kind, &[u8], &[u8])]) -> (Release, Source, Server) {
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
        assets: Default::default(),
    };
    (release, files(), server)
}

/// Each medium is kept only when it is the size and digest the signed notes name: one that is
/// not is refused and the next is tried; the words come through either way.
#[test]
fn a_medium_that_does_not_match_its_digest_is_refused_and_the_words_still_come() {
    use whats_new::{fetch_whats_new, Kind};
    let agent = agent();
    let folder = tempfile::tempdir().unwrap();
    // The first picture was changed on the server; the clip after it matches.
    let (release, source, _server) = whats_new_release(&[
        (Kind::Image, b"the picture", b"another picture"),
        (Kind::Video, b"the clip", b"the clip"),
    ]);
    let (notes, kept) = fetch_whats_new(&agent, &source, &release, folder.path()).unwrap();
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
    let (release, source, _server) =
        whats_new_release(&[(Kind::Image, b"the picture", b"the picture, longer")]);
    let (notes, kept) = fetch_whats_new(&agent, &source, &release, folder.path()).unwrap();
    assert_eq!((notes.version, kept), (version("2.0.0"), None));
    // The notes themselves changed after signing: refused whole.
    let (release, source, server) = whats_new_release(&[]);
    server.put(
        &format!("/{}", whats_new::FILE),
        b"{\"version\":\"2.0.0\"}".to_vec(),
    );
    assert_eq!(
        fetch_whats_new(&agent, &source, &release, folder.path()),
        Err(Error::Digest)
    );
    // Another release's notes, signed as this one's.
    let (release, source, _server) = whats_new_release(&[]);
    let release = Release {
        version: version("3.0.0"),
        ..release
    };
    assert_eq!(
        fetch_whats_new(&agent, &source, &release, folder.path()),
        Err(Error::Manifest)
    );
}
