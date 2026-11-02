//! A skill as a file the person picks or saves: a `.md` that is its SKILL.md, or a `.zip`
//! (`.skill` is the same) of its folder. Unpacked into a folder of its own before quill's
//! skills take it (global.rs), and never outside that folder.

use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

/// The most an archive may unpack to: a skill is words and a few scripts.
const MOST: u64 = 32 << 20;

/// Unpack the skill in `from` into `to`, a folder that does not exist yet, and say its name:
/// the frontmatter's `name`, else its folder's or file's own, as a skill's name is written.
pub fn unpack(from: &Path, to: &Path) -> Result<String, String> {
    let kind = (from.extension())
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let stem = (from.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    let folder = match kind.as_str() {
        "md" => {
            let text = std::fs::read_to_string(from).map_err(|e| e.to_string())?;
            let text = if text.starts_with("---") {
                text
            } else {
                format!("---\nname: {}\ndescription: \n---\n\n{text}", slug(&stem))
            };
            std::fs::write(to.join("SKILL.md"), text).map_err(|e| e.to_string())?;
            stem
        }
        "zip" | "skill" => unzip(from, to)?.unwrap_or(stem),
        _ => return Err("A skill is a .md file, or a .zip or .skill of its folder".into()),
    };
    let text = std::fs::read_to_string(to.join("SKILL.md"))
        .map_err(|_| "There is no SKILL.md in it".to_owned())?;
    let name = crate::global::frontmatter(&text, "name").unwrap_or(folder);
    Ok(slug(&name))
}

/// The archive's skill into `to`: the folder that holds its SKILL.md (the top or one folder
/// in), whose name is said when it has one.
fn unzip(from: &Path, to: &Path) -> Result<Option<String>, String> {
    let file = std::fs::File::open(from).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let names: Vec<PathBuf> = (0..archive.len())
        .filter_map(|i| {
            archive
                .by_index(i)
                .ok()?
                .enclosed_name()
                .map(Path::to_owned)
        })
        .collect();
    let skill = (names.iter())
        .filter(|n| n.file_name().is_some_and(|f| f == "SKILL.md"))
        .min_by_key(|n| n.components().count())
        .ok_or("There is no SKILL.md in it")?;
    let base = skill.parent().unwrap_or(Path::new("")).to_owned();
    let mut total = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        // A name that would lead out of the folder is skipped, as one under another folder.
        let Some(inside) =
            (entry.enclosed_name()).and_then(|n| n.strip_prefix(&base).ok().map(Path::to_owned))
        else {
            continue;
        };
        if inside.as_os_str().is_empty()
            || !inside
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
        {
            continue;
        }
        let target = to.join(&inside);
        if entry.is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            continue;
        }
        total += entry.size();
        if total > MOST {
            return Err("It unpacks to more than 32 MB".into());
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut bytes = vec![];
        (&mut entry)
            .take(MOST)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        std::fs::write(&target, bytes).map_err(|e| e.to_string())?;
    }
    Ok(base.file_name().map(|n| n.to_string_lossy().into_owned()))
}

/// `dir` as a `.zip` at `to`, its files under `name/`.
pub fn zip_folder(dir: &Path, name: &str, to: &Path) -> Result<(), String> {
    let file = std::fs::File::create(to).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::FileOptions::default();
    let mut stack = vec![dir.to_owned()];
    while let Some(folder) = stack.pop() {
        for entry in std::fs::read_dir(&folder)
            .map_err(|e| e.to_string())?
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let inside = path.strip_prefix(dir).map_err(|e| e.to_string())?;
            let inside = inside.to_string_lossy().replace('\\', "/");
            zip.start_file(format!("{name}/{inside}"), options)
                .map_err(|e| e.to_string())?;
            zip.write_all(&std::fs::read(&path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        }
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// A name as a skill's is written: lowercase words joined by dashes.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').chars().take(64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("quill-skills-{tag}-{n}"))
    }

    #[test]
    fn a_zip_round_trips_and_a_name_that_leads_out_is_skipped() {
        let root = dir("zip");
        let skill = root.join("release-notes");
        std::fs::create_dir_all(skill.join("scripts")).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: release-notes\ndescription: Notes\n---\nWrite them.",
        )
        .unwrap();
        std::fs::write(skill.join("scripts/run.sh"), "echo hi").unwrap();
        let archive = root.join("out.skill");
        zip_folder(&skill, "release-notes", &archive).unwrap();
        let to = root.join("in");
        assert_eq!(unpack(&archive, &to).unwrap(), "release-notes");
        assert_eq!(
            std::fs::read_to_string(to.join("scripts/run.sh")).unwrap(),
            "echo hi"
        );
        // An archive whose entry climbs out of the folder writes nothing there.
        let evil = root.join("evil.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&evil).unwrap());
        let options = zip::write::FileOptions::default();
        zip.start_file("SKILL.md", options).unwrap();
        zip.write_all(b"---\nname: evil\n---\n").unwrap();
        zip.start_file("../escaped.txt", options).unwrap();
        zip.write_all(b"no").unwrap();
        zip.finish().unwrap();
        let to = root.join("evil-in");
        assert_eq!(unpack(&evil, &to).unwrap(), "evil");
        assert!(!root.join("escaped.txt").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_markdown_file_is_a_skill_named_by_its_file_without_frontmatter() {
        let root = dir("md");
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("Code Review.md");
        std::fs::write(&file, "Check the diff.").unwrap();
        let to = root.join("in");
        assert_eq!(unpack(&file, &to).unwrap(), "code-review");
        assert!(std::fs::read_to_string(to.join("SKILL.md"))
            .unwrap()
            .contains("name: code-review"));
        assert!(unpack(&root.join("notes.txt"), &root.join("x")).is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
