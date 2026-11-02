//! A file server on the loopback interface, for the tests and the local test feed
//! (`tools/quill-release`): HTTP/1.1 `GET`, one request per connection, `Range: bytes={start}-`,
//! and faults to try a client against: a connection cut partway through a body, a slow body,
//! no ranges. Never for anything but this machine.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// What the server holds and how it misbehaves.
#[derive(Clone, Debug, Default)]
pub struct Files {
    /// By path, `/quill-update.json` and so on.
    pub files: HashMap<String, Arc<Vec<u8>>>,
    /// Cut the first body sent for this path at this many bytes; later requests are served whole.
    pub cut: Option<(String, u64)>,
    /// Send bodies in 64 KiB pieces, waiting this long before each.
    pub pace: Option<Duration>,
    /// Ignore `Range` and always send the whole file.
    pub no_ranges: bool,
}

/// Every request's path and `Range` start, in order.
type Requests = Arc<Mutex<Vec<(String, Option<u64>)>>>;

/// A running server; it stops accepting when dropped.
pub struct Server {
    pub address: SocketAddr,
    files: Arc<Mutex<Files>>,
    requests: Requests,
    stop: Arc<AtomicBool>,
    /// The throwaway key [`publish`] signs with.
    seed: [u8; 32],
}

impl Server {
    /// `http://127.0.0.1:{port}`.
    pub fn base(&self) -> String {
        format!("http://{}", self.address)
    }

    /// Serve `bytes` at `path` from now on.
    pub fn put(&self, path: &str, bytes: Vec<u8>) {
        let mut files = self.files.lock().unwrap();
        files.files.insert(path.to_owned(), Arc::new(bytes));
    }

    /// Change how it misbehaves.
    pub fn configure(&self, change: impl FnOnce(&mut Files)) {
        change(&mut self.files.lock().unwrap());
    }

    pub fn requests(&self) -> Vec<(String, Option<u64>)> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes the accept loop, which then sees the flag.
        let _ = TcpStream::connect(self.address);
    }
}

/// Serve `manifest` and its signature on `server`, signed with the server's throwaway key: the
/// key a check verifies it with, the same for every manifest the server publishes.
pub fn publish(server: &Server, manifest: &crate::Manifest) -> ed25519_dalek::VerifyingKey {
    use base64::Engine as _;
    use ed25519_dalek::Signer as _;
    let key = ed25519_dalek::SigningKey::from_bytes(&server.seed);
    let bytes = serde_json::to_vec(manifest).unwrap();
    let signature = base64::engine::general_purpose::STANDARD.encode(key.sign(&bytes).to_bytes());
    server.put(&format!("/{}", crate::SIGNATURE), signature.into_bytes());
    server.put(&format!("/{}", crate::MANIFEST), bytes);
    key.verifying_key()
}

/// Serve the bundle unpacked in the folder `tree` file by file, as `version` on `platform`
/// publishes it: its blobs and its list ([`crate::files`]). Returns the list's entry for the
/// manifest's `files`.
pub fn publish_files(
    server: &Server,
    tree: &std::path::Path,
    version: &crate::Version,
    platform: &str,
) -> crate::Artifact {
    use sha2::Digest as _;
    let out = std::env::temp_dir().join(format!(
        "quill-feed-{}-{}",
        std::process::id(),
        server.address.port()
    ));
    let (bundle, blobs) = crate::files::pack(
        tree,
        version,
        platform,
        &server.base(),
        &Default::default(),
        &out,
    )
    .expect("the bundle's folder");
    for blob in blobs {
        let name = blob.file_name().unwrap().to_string_lossy();
        server.put(&format!("/{name}"), std::fs::read(&blob).unwrap());
    }
    let _ = std::fs::remove_dir_all(&out);
    let bytes = serde_json::to_vec(&bundle).unwrap();
    let name = crate::files::index_name(platform);
    let entry = crate::Artifact {
        url: format!("{}/{name}", server.base()),
        size: bytes.len() as u64,
        sha256: crate::hex(&sha2::Sha256::digest(&bytes)),
    };
    server.put(&format!("/{name}"), bytes);
    entry
}

/// Serve on `127.0.0.1:{port}` (0: any free port), with nothing yet.
pub fn serve(port: u16) -> std::io::Result<Server> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let address = listener.local_addr()?;
    let files = Arc::new(Mutex::new(Files::default()));
    let requests: Requests = Arc::new(Mutex::new(Vec::new()));
    let cut = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    let (shared, seen, halt) = (files.clone(), requests.clone(), stop.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if halt.load(Ordering::SeqCst) {
                break;
            }
            let Ok(stream) = stream else { continue };
            let (files, seen, cut) = (shared.clone(), seen.clone(), cut.clone());
            std::thread::spawn(move || {
                let _ = answer(stream, &files, &seen, &cut);
            });
        }
    });
    // From std's own random hasher keys: no randomness crate for a test key.
    let mut seed = [0; 32];
    for (at, piece) in seed.chunks_mut(8).enumerate() {
        use std::hash::BuildHasher as _;
        let random = std::collections::hash_map::RandomState::new().hash_one(at);
        piece.copy_from_slice(&random.to_le_bytes());
    }
    Ok(Server {
        address,
        files,
        requests,
        stop,
        seed,
    })
}

fn answer(
    mut stream: TcpStream,
    files: &Mutex<Files>,
    seen: &Requests,
    cut: &AtomicBool,
) -> std::io::Result<()> {
    let mut head = Vec::new();
    let mut byte = [0; 1];
    while !head.ends_with(b"\r\n\r\n") && head.len() < 8192 {
        if stream.read(&mut byte)? == 0 {
            return Ok(());
        }
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head);
    let path = head
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .unwrap_or("/")
        .to_owned();
    let range = head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if !name.eq_ignore_ascii_case("range") {
            return None;
        }
        value
            .trim()
            .strip_prefix("bytes=")?
            .strip_suffix('-')?
            .parse::<u64>()
            .ok()
    });
    seen.lock().unwrap().push((path.clone(), range));
    let (file, limit, pace) = {
        let files = files.lock().unwrap();
        let limit = files
            .cut
            .as_ref()
            .filter(|(cut_path, _)| *cut_path == path)
            .map(|(_, limit)| *limit);
        let range = range.filter(|_| !files.no_ranges);
        (
            files.files.get(&path).cloned().map(|file| (file, range)),
            limit,
            files.pace,
        )
    };
    let Some((file, range)) = file else {
        return stream.write_all(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
    };
    let size = file.len() as u64;
    let start = range.filter(|start| *start < size).unwrap_or(0);
    let mut header = if start > 0 {
        format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{}/{size}\r\n",
            size - 1
        )
    } else {
        "HTTP/1.1 200 OK\r\n".to_owned()
    };
    header.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        size - start
    ));
    stream.write_all(header.as_bytes())?;
    let mut body = &file[start as usize..];
    if let Some(limit) = limit {
        if !cut.swap(true, Ordering::SeqCst) {
            body = &body[..(limit as usize).min(body.len())];
        }
    }
    for piece in body.chunks(64 * 1024) {
        if let Some(pace) = pace {
            std::thread::sleep(pace);
        }
        stream.write_all(piece)?;
    }
    stream.flush()
}
