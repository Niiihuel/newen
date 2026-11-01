//! A local feed of a made-up release, signed with a throwaway key, to try
//! quill's updates before there is a real one (`docs/UPDATES.md`):
//!
//! ```text
//! cargo run -p quill-release --bin quill-test-feed -- \
//!     [--version 9.9.9] [--mandatory] [--slow] [--corrupt] [--port 8765] [--artifact <file>]
//!     [--platform windows-x86_64]
//! ```
//!
//! It prints the `QUILL_UPDATE_FEED` and `QUILL_UPDATE_KEY` a debug build of
//! quill needs to use it, and serves until it is stopped. `--artifact` is
//! what the release installs, made as the release workflow makes it (Linux's
//! bundle as a `.tar.gz`, Windows' and macOS' as a `.zip`) from a build of
//! quill with that version; without it the artifact is a stand-in that is
//! not an archive, which installing refuses. `--slow` paces the download,
//! `--corrupt` signs a digest the artifact does not have. `--platform` serves
//! another system than this one (a quill under Wine updating from a feed on
//! Linux).

use std::time::Duration;

use base64::Engine as _;
use ed25519_dalek::{Signer, SigningKey};
use quill_update::{Artifact, Manifest, Version};
use sha2::{Digest, Sha256};

fn main() {
    let mut arguments = std::env::args().skip(1);
    let (mut version, mut mandatory, mut slow, mut corrupt) =
        ("9.9.9".to_owned(), false, false, false);
    let (mut port, mut artifact, mut platform) = (8765, None, quill_update::platform());
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--version" => version = arguments.next().expect("--version <x.y.z>"),
            "--mandatory" => mandatory = true,
            "--slow" => slow = true,
            "--corrupt" => corrupt = true,
            "--port" => {
                port = arguments
                    .next()
                    .and_then(|port| port.parse().ok())
                    .expect("--port <n>")
            }
            "--artifact" => artifact = Some(arguments.next().expect("--artifact <file>")),
            "--platform" => platform = arguments.next().expect("--platform <os-arch>"),
            other => panic!("unknown argument {other}"),
        }
    }
    let version = Version::parse(&version).expect("a version");
    let artifact = match artifact {
        Some(file) => std::fs::read(file).expect("the artifact"),
        // A stand-in that does not compress, 8 MiB, so its progress shows.
        None => {
            let mut state = 0x9e37_79b9_7f4a_7c15_u64;
            (0..8 << 20)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    state as u8
                })
                .collect()
        }
    };
    // As the release workflow names them.
    let name = match platform.split('-').next() {
        Some("windows") => "quill-x86_64-windows.zip",
        Some("macos") => "quill-aarch64-macos.zip",
        _ => "quill-x86_64-linux.tar.gz",
    };

    let mut seed = [0; 32];
    getrandom::fill(&mut seed).unwrap();
    let key = SigningKey::from_bytes(&seed);
    let server = quill_update::serve::serve(port).expect("the port is free");
    let mut sha256 = quill_update::hex(&Sha256::digest(&artifact));
    if corrupt {
        sha256 = quill_update::hex(&Sha256::digest(b"something else"));
    }
    let manifest = Manifest {
        minimum_version: mandatory.then(|| version.clone()),
        version,
        date: "2026-10-19T12:00:00-03:00".into(),
        notes: "A test release of the local feed.\n\n- Fixes the start from the cache.\n- The member list opens faster.".into(),
        platforms: [(
            platform,
            Artifact {
                url: format!("{}/{name}", server.base()),
                size: artifact.len() as u64,
                sha256,
            },
        )]
        .into(),
    };
    let bytes = serde_json::to_vec_pretty(&manifest).unwrap();
    let signature = base64::engine::general_purpose::STANDARD.encode(key.sign(&bytes).to_bytes());
    server.put(
        &format!("/{}", quill_update::SIGNATURE),
        signature.into_bytes(),
    );
    server.put(&format!("/{}", quill_update::MANIFEST), bytes);
    server.put(&format!("/{name}"), artifact);
    if slow {
        server.configure(|files| files.pace = Some(Duration::from_millis(150)));
    }
    println!("QUILL_UPDATE_FEED={}", server.base());
    println!(
        "QUILL_UPDATE_KEY={}",
        base64::engine::general_purpose::STANDARD.encode(key.verifying_key().to_bytes())
    );
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}
