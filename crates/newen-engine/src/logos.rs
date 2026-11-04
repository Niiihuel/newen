//! The logos of connectors, plugins and model providers (docs/engine.md): looked up by name on
//! svgl.app, else at the logo address its catalogue gives (a provider's), else the server's own
//! site, and kept as PNG files under Newen's data folder. Blocking: it fetches,
//! decodes and rasterizes, so it runs on a blocking thread. Newen ships no logo.

use newen_model::LogoWanted;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Each logo is this many pixels on a side.
const SIDE: u32 = 64;
/// The most a response may be.
const LIMIT: u64 = 512 * 1024;
/// A "nothing found" marker is kept this long before the lookup is tried again.
const RETRY_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// The light and dark logo of `wanted` in `dir`: from disk when kept, else looked up and kept.
/// `None` for a side with no logo; a lookup that finds none is kept as a marker.
pub fn logo(dir: &Path, wanted: &LogoWanted) -> (Option<PathBuf>, Option<PathBuf>) {
    let slug = slug(&wanted.key);
    let light = dir.join(format!("{slug}-light.png"));
    let dark = dir.join(format!("{slug}-dark.png"));
    let none = dir.join(format!("{slug}.none"));
    if light.is_file() {
        let dark = if dark.is_file() { dark } else { light.clone() };
        return (Some(light), Some(dark));
    }
    let marked = std::fs::metadata(&none)
        .and_then(|meta| meta.modified())
        .is_ok_and(|written| fresh(written, SystemTime::now()));
    if marked {
        return (None, None);
    }
    let _ = std::fs::create_dir_all(dir);
    let kept = find(wanted).is_some_and(|(light_png, dark_png)| {
        std::fs::write(&light, light_png).is_ok() && std::fs::write(&dark, dark_png).is_ok()
    });
    if kept {
        let _ = std::fs::remove_file(&none);
        (Some(light), Some(dark))
    } else {
        let _ = std::fs::write(&none, b"");
        (None, None)
    }
}

/// A file name for `key`: its letters and digits, lower case, and a dash for anything else.
fn slug(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

/// Whether a marker written at `written` still stands at `now`. A time in the future counts
/// as fresh.
fn fresh(written: SystemTime, now: SystemTime) -> bool {
    now.duration_since(written)
        .map_or(true, |age| age < RETRY_AFTER)
}

/// The light and dark PNGs for `wanted`, from svgl.app by name, else from its `url`: the logo
/// itself when that is an SVG (a catalogue's), else the server's site.
fn find(wanted: &LogoWanted) -> Option<(Vec<u8>, Vec<u8>)> {
    let agent = agent();
    let svg = (wanted.url.as_deref()).filter(|url| url.ends_with(".svg"));
    svgl(&agent, &wanted.name).or_else(|| match svg {
        Some(url) => catalogue(&agent, url),
        None => site(&agent, wanted.url.as_deref()),
    })
}

/// The ink a one-colour logo is drawn in on a dark tile.
const DARK_INK: &str = "#ececec";

/// A catalogue's SVG logo, rasterized for each tile: as drawn for a light one, and for a dark
/// one in a light ink when it is one colour (`currentColor`, or none given), which would vanish
/// drawn black.
fn catalogue(agent: &ureq::Agent, url: &str) -> Option<(Vec<u8>, Vec<u8>)> {
    let svg = String::from_utf8(get(agent, url, None)?).ok()?;
    let light = rasterize(svg.as_bytes())?;
    let dark = rasterize(dark_ink(&svg).as_bytes()).unwrap_or_else(|| light.clone());
    Some((light, dark))
}

/// `svg` with its one colour turned light: `currentColor`, or the black it is drawn in when it
/// names no fill. One that gives its own colours stays as it is.
fn dark_ink(svg: &str) -> String {
    let inked = svg.replace("currentColor", DARK_INK);
    if inked != svg || svg.contains("fill=") {
        return inked;
    }
    inked.replacen("<svg", &format!("<svg fill=\"{DARK_INK}\""), 1)
}

/// svgl.app's logo for a name: its light and dark vectors, each rasterized.
fn svgl(agent: &ureq::Agent, name: &str) -> Option<(Vec<u8>, Vec<u8>)> {
    let body = get(agent, "https://api.svgl.app", Some(("search", name)))?;
    let (light, dark) = svgl_routes(&String::from_utf8(body).ok()?, name)?;
    let light_png = vector(agent, &light)?;
    let dark_png = if dark == light {
        light_png.clone()
    } else {
        vector(agent, &dark).unwrap_or_else(|| light_png.clone())
    };
    Some((light_png, dark_png))
}

/// The light and dark addresses of the svgl entry titled `name`, its letters and digits in any
/// case ("DigitalOcean" is "Digital Ocean"), from its search answer. An error or a list
/// without that title gives none; a plain route serves both.
fn svgl_routes(body: &str, name: &str) -> Option<(String, String)> {
    let entries: Vec<serde_json::Value> = serde_json::from_str(body).ok()?;
    let key = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    };
    let name = key(name);
    let entry = (entries.iter()).find(|e| e["title"].as_str().is_some_and(|t| key(t) == name))?;
    match &entry["route"] {
        serde_json::Value::String(url) => Some((url.clone(), url.clone())),
        route => {
            let light = route["light"].as_str()?;
            Some((
                light.to_owned(),
                route["dark"].as_str().unwrap_or(light).to_owned(),
            ))
        }
    }
}

/// The server's own site icon, Apple's touch icon first, then `favicon.ico`.
fn site(agent: &ureq::Agent, url: Option<&str>) -> Option<(Vec<u8>, Vec<u8>)> {
    let domain = site_domain(url?)?;
    ["apple-touch-icon.png", "favicon.ico"]
        .iter()
        .find_map(|file| {
            let bytes = get(agent, &format!("https://{domain}/{file}"), None)?;
            let png = bitmap(&bytes)?;
            Some((png.clone(), png))
        })
}

/// The domain whose icons a server's address names: its host, without a leading `mcp.` or
/// `api.` label (`https://mcp.context7.com/mcp` gives `context7.com`).
fn site_domain(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority
        .rsplit('@')
        .next()?
        .split(':')
        .next()?
        .to_lowercase();
    let host = host
        .strip_prefix("mcp.")
        .or_else(|| host.strip_prefix("api."))
        .unwrap_or(&host);
    let plain = host.contains('.')
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    plain.then(|| host.to_owned())
}

/// An SVG at `url`, rasterized.
fn vector(agent: &ureq::Agent, url: &str) -> Option<Vec<u8>> {
    rasterize(&get(agent, url, None)?)
}

/// An SVG drawn into a transparent square, as PNG. Its own references to other files are not
/// followed: only data URLs its picture may carry are drawn.
fn rasterize(svg: &[u8]) -> Option<Vec<u8>> {
    let mut options = resvg::usvg::Options::default();
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    let tree = resvg::usvg::Tree::from_data(svg, &options).ok()?;
    let size = tree.size();
    let scale = SIDE as f32 / size.width().max(size.height());
    let shift = |side: f32| (SIDE as f32 - side * scale) / 2.0;
    let transform = resvg::tiny_skia::Transform::from_row(
        scale,
        0.0,
        0.0,
        scale,
        shift(size.width()),
        shift(size.height()),
    );
    let mut pixmap = resvg::tiny_skia::Pixmap::new(SIDE, SIDE)?;
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap.encode_png().ok()
}

/// A PNG or ICO image, scaled into a transparent square as PNG; other formats are refused.
fn bitmap(bytes: &[u8]) -> Option<Vec<u8>> {
    let format = image::guess_format(bytes).ok()?;
    if !matches!(format, image::ImageFormat::Png | image::ImageFormat::Ico) {
        return None;
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    // A small file can decode to a huge picture: the sizes are capped before decoding.
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(2048);
    limits.max_alloc = Some(64 << 20);
    reader.limits(limits);
    let picture = reader.decode().ok()?.thumbnail(SIDE, SIDE).to_rgba8();
    let mut square = image::RgbaImage::new(SIDE, SIDE);
    let at = |side: u32| i64::from((SIDE - side) / 2);
    image::imageops::overlay(
        &mut square,
        &picture,
        at(picture.width()),
        at(picture.height()),
    );
    let mut png = Cursor::new(Vec::new());
    square.write_to(&mut png, image::ImageFormat::Png).ok()?;
    Some(png.into_inner())
}

/// The agent every lookup uses: https only, ten seconds at most.
pub(crate) fn agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .https_only(true)
            .timeout_global(Some(Duration::from_secs(10)))
            .build(),
    )
}

/// The body of `url` (with one query pair), at most `LIMIT` bytes; `None` on any failure.
pub(crate) fn get(agent: &ureq::Agent, url: &str, query: Option<(&str, &str)>) -> Option<Vec<u8>> {
    let mut request = agent.get(url);
    if let Some((key, value)) = query {
        request = request.query(key, value);
    }
    let mut response = request.call().ok()?;
    response
        .body_mut()
        .with_config()
        .limit(LIMIT)
        .read_to_vec()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svgl_takes_the_entry_titled_exactly_as_asked() {
        let plain = r#"[{"title":"Figma","route":"https://svgl.app/library/figma.svg"}]"#;
        assert_eq!(
            svgl_routes(plain, "figma"),
            Some((
                "https://svgl.app/library/figma.svg".into(),
                "https://svgl.app/library/figma.svg".into()
            ))
        );
        let themed = r#"[{"title":"Vercel","route":{"light":"https://svgl.app/library/vercel.svg","dark":"https://svgl.app/library/vercel_dark.svg"}}]"#;
        assert_eq!(
            svgl_routes(themed, "vercel"),
            Some((
                "https://svgl.app/library/vercel.svg".into(),
                "https://svgl.app/library/vercel_dark.svg".into()
            ))
        );
        // "Google" is not "Google Drive": no partial match.
        let partial = r#"[{"title":"Google","route":"https://svgl.app/library/google.svg"}]"#;
        assert_eq!(svgl_routes(partial, "Google Drive"), None);
        assert!(svgl_routes(partial, "GOOGLE").is_some());
    }

    #[test]
    fn svgl_error_answers_give_none() {
        assert_eq!(svgl_routes(r#"{"error":"not found"}"#, "context7"), None);
        assert_eq!(svgl_routes("not json", "figma"), None);
    }

    #[test]
    fn a_server_address_names_its_site() {
        assert_eq!(
            site_domain("https://mcp.context7.com/mcp").as_deref(),
            Some("context7.com")
        );
        assert_eq!(
            site_domain("https://mcp.figma.com/mcp").as_deref(),
            Some("figma.com")
        );
        assert_eq!(site_domain("http://example.com/mcp"), None);
        assert_eq!(site_domain("https://localhost:8080/mcp"), None);
    }

    #[test]
    fn a_marker_stands_for_a_week() {
        let now = SystemTime::now();
        assert!(fresh(now - Duration::from_secs(60), now));
        assert!(fresh(now + Duration::from_secs(60), now));
        assert!(!fresh(now - RETRY_AFTER - Duration::from_secs(1), now));
    }

    #[test]
    fn a_one_colour_logo_is_inked_light_for_a_dark_tile() {
        let current = r#"<svg viewBox="0 0 1 1"><path fill="currentColor" d="M0 0h1v1z"/></svg>"#;
        assert!(dark_ink(current).contains(&format!("fill=\"{DARK_INK}\"")));
        let bare = r#"<svg viewBox="0 0 1 1"><path d="M0 0h1v1z"/></svg>"#;
        assert!(dark_ink(bare).starts_with(&format!("<svg fill=\"{DARK_INK}\"")));
        let coloured = r##"<svg viewBox="0 0 1 1"><path fill="#ff5500" d="M0 0h1v1z"/></svg>"##;
        assert_eq!(dark_ink(coloured), coloured);
    }

    #[test]
    fn an_svgl_title_matches_by_its_letters_and_digits() {
        let body = r#"[{"title":"Digital Ocean","route":"https://svgl.app/do.svg"},
            {"title":"Vercel","route":{"light":"https://svgl.app/v-l.svg","dark":"https://svgl.app/v-d.svg"}}]"#;
        assert_eq!(
            svgl_routes(body, "DigitalOcean"),
            Some((
                "https://svgl.app/do.svg".into(),
                "https://svgl.app/do.svg".into()
            ))
        );
        assert_eq!(
            svgl_routes(body, "vercel").unwrap().1,
            "https://svgl.app/v-d.svg"
        );
        assert_eq!(
            svgl_routes(body, "Vercel AI Gateway"),
            None,
            "not a part of a name"
        );
    }

    #[test]
    fn a_kept_logo_is_answered_from_disk() {
        let dir = std::env::temp_dir().join(format!("newen-logos-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mcp-figma-light.png"), b"png").unwrap();
        let wanted = LogoWanted {
            key: "mcp:figma".into(),
            name: "figma".into(),
            url: Some("https://mcp.figma.com/mcp".into()),
        };
        let (light, dark) = logo(&dir, &wanted);
        assert_eq!(light, Some(dir.join("mcp-figma-light.png")));
        assert_eq!(dark, light);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_fresh_marker_is_answered_without_a_lookup() {
        let dir = std::env::temp_dir().join(format!("newen-logos-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("plugin-nothing.none"), b"").unwrap();
        let wanted = LogoWanted {
            key: "plugin:nothing".into(),
            name: "nothing".into(),
            url: None,
        };
        assert_eq!(logo(&dir, &wanted), (None, None));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// One real lookup over the network: `cargo test -p newen-engine live_lookup -- --ignored`.
    #[test]
    #[ignore]
    fn live_lookup_finds_figma_vercel_and_context7() {
        let dir = std::env::temp_dir().join(format!("newen-logos-live-{}", std::process::id()));
        let cases = [
            ("plugin:figma", "figma", None),
            ("plugin:vercel", "vercel", None),
            (
                "mcp:context7",
                "context7",
                Some("https://mcp.context7.com/mcp"),
            ),
            // A provider svgl has, and one only its catalogue draws.
            (
                "provider:Groq",
                "Groq",
                Some("https://models.dev/logos/groq.svg"),
            ),
            (
                "provider:302.AI",
                "302.AI",
                Some("https://models.dev/logos/302ai.svg"),
            ),
        ];
        for (key, name, url) in cases {
            let wanted = LogoWanted {
                key: key.into(),
                name: name.into(),
                url: url.map(str::to_owned),
            };
            let (light, dark) = logo(&dir, &wanted);
            println!("{key}: light {light:?} dark {dark:?}");
            assert!(light.is_some(), "{key} found no logo");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
