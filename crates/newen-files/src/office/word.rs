//! A Word document (.docx) as Markdown: its header, body and footer; paragraphs, headings, lists
//! with their numbers, bold and italic, line breaks, tables, and where an image is.
use super::{inline, line_start, table};
use roxmltree::Node;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

/// WordprocessingML's namespace, and the one of a part's relationships.
const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

pub(super) fn reads(path: &Path) -> bool {
    super::extension(path) == "docx"
}

pub(super) fn markdown(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut parts = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut part = |name: &str| -> Option<String> {
        let mut xml = String::new();
        parts.by_name(name).ok()?.read_to_string(&mut xml).ok()?;
        Some(xml)
    };
    let body = part("word/document.xml").ok_or("This document has no body.")?;
    let styles = styles(&part("word/styles.xml").unwrap_or_default());
    let lists = lists(&part("word/numbering.xml").unwrap_or_default());
    let links = links(&part("word/_rels/document.xml.rels").unwrap_or_default());
    let body = roxmltree::Document::parse(&body).map_err(|e| e.to_string())?;
    let Some(body) = body.descendants().find(|n| n.has_tag_name((W, "body"))) else {
        return Ok(String::new());
    };
    // A document with a title has its headings one level under it.
    let titled = (body.descendants())
        .filter(|n| n.has_tag_name((W, "p")))
        .any(|p| style_of(p, &styles).is_some_and(|s| s.title));
    let write = |root: Node| {
        let mut writer = Writer {
            styles: &styles,
            lists: &lists,
            counters: HashMap::new(),
            shift: titled as usize,
            out: String::new(),
        };
        writer.blocks(root);
        writer.out
    };
    // The header and footer of its last section, the one most pages have.
    let mut edge = |kind: &str| -> String {
        let id = (child(body, "sectPr").into_iter())
            .flat_map(|section| section.children())
            .find(|n| n.has_tag_name((W, kind)) && n.attribute((W, "type")) == Some("default"))
            .and_then(|n| n.attribute((R, "id")));
        let target = id.and_then(|id| links.iter().find(|(link, _)| link == id));
        let Some(xml) = target.and_then(|(_, target)| part(&format!("word/{target}"))) else {
            return String::new();
        };
        roxmltree::Document::parse(&xml).map_or_else(|_| String::new(), |d| write(d.root_element()))
    };
    let (header, footer) = (edge("headerReference"), edge("footerReference"));
    let mut out = String::new();
    if !header.trim().is_empty() {
        out += &format!("{header}---\n\n");
    }
    out += &write(body);
    if !footer.trim().is_empty() {
        out += &format!("---\n\n{footer}");
    }
    Ok(out)
}

/// What a paragraph style makes of its paragraphs.
struct Style {
    id: String,
    /// A built-in style keeps its English name ("heading 2", "Title") whatever language Word
    /// speaks, where its id is translated; another has its outline level.
    heading: Option<usize>,
    title: bool,
    /// The list a list style ("List Bullet") puts its paragraphs in, and at what level.
    list: Option<(String, usize)>,
}

fn styles(xml: &str) -> Vec<Style> {
    let Ok(styles) = roxmltree::Document::parse(xml) else {
        return vec![];
    };
    (styles.descendants())
        .filter(|n| n.has_tag_name((W, "style")))
        .filter_map(|style| {
            let id = style.attribute((W, "styleId"))?.to_owned();
            let name = child(style, "name").and_then(|n| n.attribute((W, "val")));
            let name = name.unwrap_or_default().to_ascii_lowercase();
            let properties = child(style, "pPr");
            let value = |path: &[&str]| -> Option<&str> {
                let mut node = properties?;
                for name in path {
                    node = child(node, name)?;
                }
                node.attribute((W, "val"))
            };
            let outline = value(&["outlineLvl"]).and_then(|n| n.parse::<usize>().ok());
            let heading = match name.as_str() {
                "title" => Some(1),
                name => (name.strip_prefix("heading "))
                    .and_then(|n| n.parse().ok())
                    .or(outline.map(|n| n + 1)),
            };
            let level = value(&["numPr", "ilvl"]).and_then(|l| l.parse().ok());
            Some(Style {
                id,
                heading,
                title: name == "title",
                list: (value(&["numPr", "numId"]).map(|id| (id.to_owned(), level.unwrap_or(0)))),
            })
        })
        .collect()
}

/// Each list by its id: for each level, whether it counts (`1.`, else a bullet) and from where.
fn lists(xml: &str) -> HashMap<String, Vec<(bool, usize)>> {
    let Ok(numbering) = roxmltree::Document::parse(xml) else {
        return HashMap::new();
    };
    fn val<'a>(node: Node<'a, 'a>, name: &str) -> Option<&'a str> {
        child(node, name)?.attribute((W, "val"))
    }
    let number = |node: Node, name: &str| val(node, name).and_then(|v| v.parse::<usize>().ok());
    let mut abstracts = HashMap::new();
    for list in (numbering.descendants()).filter(|n| n.has_tag_name((W, "abstractNum"))) {
        let mut levels = vec![(false, 1); 9];
        for level in list.children().filter(|n| n.has_tag_name((W, "lvl"))) {
            let at = (level.attribute((W, "ilvl"))).and_then(|l| l.parse::<usize>().ok());
            if let Some(slot) = at.and_then(|at| levels.get_mut(at)) {
                let format = val(level, "numFmt").unwrap_or("bullet");
                *slot = (
                    !matches!(format, "bullet" | "none"),
                    number(level, "start").unwrap_or(1),
                );
            }
        }
        if let Some(id) = list.attribute((W, "abstractNumId")) {
            abstracts.insert(id.to_owned(), levels);
        }
    }
    let mut lists = HashMap::new();
    for list in (numbering.descendants()).filter(|n| n.has_tag_name((W, "num"))) {
        let base = val(list, "abstractNumId").and_then(|id| abstracts.get(id));
        let (Some(id), Some(mut levels)) = (list.attribute((W, "numId")), base.cloned()) else {
            continue;
        };
        // A list may start one of its levels elsewhere.
        for over in list
            .children()
            .filter(|n| n.has_tag_name((W, "lvlOverride")))
        {
            let at = (over.attribute((W, "ilvl"))).and_then(|l| l.parse::<usize>().ok());
            if let (Some(slot), Some(start)) = (
                at.and_then(|at| levels.get_mut(at)),
                number(over, "startOverride"),
            ) {
                slot.1 = start;
            }
        }
        lists.insert(id.to_owned(), levels);
    }
    lists
}

/// The part each relationship of the document's names: (id, target).
fn links(xml: &str) -> Vec<(String, String)> {
    let Ok(links) = roxmltree::Document::parse(xml) else {
        return vec![];
    };
    (links.descendants())
        .filter(|n| n.tag_name().name() == "Relationship")
        .filter_map(|n| Some((n.attribute("Id")?.into(), n.attribute("Target")?.into())))
        .collect()
}

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|n| n.has_tag_name((W, name)))
}

fn style_of<'s>(paragraph: Node, styles: &'s [Style]) -> Option<&'s Style> {
    let id = (child(paragraph, "pPr").and_then(|p| child(p, "pStyle")))
        .and_then(|s| s.attribute((W, "val")))?;
    styles.iter().find(|s| s.id == id)
}

struct Writer<'a> {
    styles: &'a [Style],
    lists: &'a HashMap<String, Vec<(bool, usize)>>,
    /// The number each level of each list is at.
    counters: HashMap<(String, usize), usize>,
    /// Levels the headings move down: one under a document's title.
    shift: usize,
    out: String,
}

impl Writer<'_> {
    fn blocks(&mut self, parent: Node) {
        for node in parent.children() {
            match node.tag_name().name() {
                _ if node.tag_name().namespace() != Some(W) => {}
                "p" => self.paragraph(node),
                "tbl" => self.table(node),
                // A content control holds blocks of its own.
                "sdt" => {
                    if let Some(content) = child(node, "sdtContent") {
                        self.blocks(content);
                    }
                }
                _ => {}
            }
        }
    }

    fn paragraph(&mut self, node: Node) {
        let style = style_of(node, self.styles);
        let numbering = child(node, "pPr").and_then(|p| child(p, "numPr"));
        let value = |name: &str| numbering.and_then(|n| child(n, name)?.attribute((W, "val")));
        // Its own numbering, or its style's; `numId` 0 takes away what the style gives.
        let list = match value("numId") {
            Some(id) => Some((id.to_owned(), 0)),
            None => style.and_then(|s| s.list.clone()),
        };
        let level = (value("ilvl").and_then(|l| l.parse().ok()))
            .or(list.as_ref().map(|(_, level)| *level))
            .unwrap_or(0);
        let list = list.filter(|(id, _)| id != "0");
        let heading = style.and_then(|s| {
            let level = s.heading?;
            Some(if s.title { level } else { level + self.shift })
        });
        if let (None, Some((id, _))) = (heading, &list) {
            let text = runs(node, " ");
            if text.trim().is_empty() {
                return;
            }
            let marker = match self.number(id, level) {
                Some(n) => format!("{n}."),
                None => "-".into(),
            };
            // limn reads a level from each two spaces, up to four.
            self.out += &format!("{}{marker} {}\n", "  ".repeat(level.min(4)), text.trim());
            return;
        }
        let text = runs(node, "\n");
        if text.trim().is_empty() {
            return;
        }
        self.end_list();
        self.out += &match heading {
            Some(level @ 1..=3) => format!("{} {}\n\n", "#".repeat(level), text.trim()),
            // limn's headings go to three levels: a deeper one is bold.
            Some(_) => format!("**{}**\n\n", text.trim()),
            None => {
                let lines: Vec<String> =
                    text.trim().lines().map(|l| line_start(l.trim())).collect();
                format!("{}\n\n", lines.join("  \n"))
            }
        };
    }

    /// The number of this item in its list, or `None` for a bullet; the levels under it start
    /// again.
    fn number(&mut self, list: &str, level: usize) -> Option<usize> {
        let (counts, start) = self.lists.get(list)?.get(level).copied()?;
        self.counters
            .retain(|(id, at), _| id != list || *at <= level);
        let n = (self.counters)
            .entry((list.to_owned(), level))
            .or_insert(start.saturating_sub(1));
        *n += 1;
        counts.then_some(*n)
    }

    /// A list's items end in one newline: the block after it starts after a blank line.
    fn end_list(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with("\n\n") {
            self.out += "\n";
        }
    }

    /// A table with its first row as the header, each cell's paragraphs on one line.
    fn table(&mut self, node: Node) {
        let rows: Vec<Vec<String>> = (node.children())
            .filter(|n| n.has_tag_name((W, "tr")))
            .map(|row| {
                (row.children())
                    .filter(|n| n.has_tag_name((W, "tc")))
                    .map(|cell| {
                        let words: Vec<String> = (cell.descendants())
                            .filter(|n| n.has_tag_name((W, "p")))
                            .map(|p| runs(p, " ").trim().to_owned())
                            .filter(|w| !w.is_empty())
                            .collect();
                        words.join(" ")
                    })
                    .collect()
            })
            .collect();
        let table = table(&rows);
        if !table.is_empty() {
            self.end_list();
            self.out += &table;
        }
    }
}

/// A paragraph's words, bold and italic as Markdown marks them; `newline` where it breaks a
/// line, and an image by what it says it shows.
fn runs(paragraph: Node, newline: &str) -> String {
    // Text that looks the same, together: (text, bold, italic).
    let mut pieces: Vec<(String, bool, bool)> = vec![];
    let mut add = |text: &str, bold: bool, italic: bool| match pieces.last_mut() {
        Some(last) if (last.1, last.2) == (bold, italic) => last.0 += text,
        _ => pieces.push((text.to_owned(), bold, italic)),
    };
    for run in paragraph.descendants().filter(|n| n.has_tag_name((W, "r"))) {
        let properties = child(run, "rPr");
        let on = |name: &str| {
            (properties.and_then(|p| child(p, name)))
                .is_some_and(|n| !matches!(n.attribute((W, "val")), Some("0" | "false")))
        };
        let (bold, italic) = (on("b"), on("i"));
        for part in run.children() {
            match part.tag_name().name() {
                "t" => add(&inline(part.text().unwrap_or_default()), bold, italic),
                "tab" => add(" ", bold, italic),
                "br" | "cr" => add(newline, bold, italic),
                "drawing" | "pict" => {
                    // An image by its description (alt text), else its title.
                    let said = (part.descendants())
                        .filter(|n| n.tag_name().name() == "docPr")
                        .find_map(|n| {
                            (n.attribute("descr").filter(|d| !d.trim().is_empty()))
                                .or(n.attribute("title").filter(|t| !t.trim().is_empty()))
                        });
                    let label = said.map_or("Image".into(), |s| format!("Image: {}", s.trim()));
                    add(&format!("\\[{}\\]", inline(&label)), false, true);
                }
                _ => {}
            }
        }
    }
    let mut out = String::new();
    for (text, bold, italic) in pieces {
        let mark = match (bold, italic) {
            _ if text.trim().is_empty() => "",
            (true, true) => "***",
            (true, false) => "**",
            (false, true) => "*",
            (false, false) => "",
        };
        out += &format!("{mark}{text}{mark}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::tests::zipped;
    use super::*;

    const STYLES: &str = r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:style w:styleId="Ttulo"><w:name w:val="Title"/></w:style>
        <w:style w:styleId="Ttulo1"><w:name w:val="heading 1"/></w:style>
        <w:style w:styleId="Ttulo4"><w:name w:val="heading 4"/></w:style>
        <w:style w:styleId="Mio"><w:name w:val="Mine"/><w:pPr><w:outlineLvl w:val="1"/></w:pPr></w:style>
        <w:style w:styleId="Vietas"><w:name w:val="List Bullet"/><w:pPr><w:numPr><w:numId w:val="1"/></w:numPr></w:pPr></w:style>
    </w:styles>"#;

    const NUMBERING: &str = r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum>
        <w:abstractNum w:abstractNumId="1">
            <w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/></w:lvl>
            <w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/></w:lvl>
        </w:abstractNum>
        <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
        <w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>
        <w:num w:numId="3"><w:abstractNumId w:val="1"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="7"/></w:lvlOverride></w:num>
    </w:numbering>"#;

    fn docx(name: &str, body: &str, more: &[(&str, &str)]) -> std::path::PathBuf {
        let document = format!(
            r#"<w:document xmlns:w="{W}" xmlns:r="{R}" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><w:body>{body}</w:body></w:document>"#
        );
        let mut parts = vec![
            ("word/document.xml", document.as_str()),
            ("word/styles.xml", STYLES),
            ("word/numbering.xml", NUMBERING),
        ];
        parts.extend_from_slice(more);
        zipped(name, &parts)
    }

    fn p(properties: &str, runs: &str) -> String {
        format!("<w:p><w:pPr>{properties}</w:pPr>{runs}</w:p>")
    }

    fn r(text: &str) -> String {
        format!(r#"<w:r><w:t xml:space="preserve">{text}</w:t></w:r>"#)
    }

    /// Headings by their style's English name (its id is translated) and outline level, under
    /// the title; bold and italic; an image; a line break; text that looks like a block; a table.
    #[test]
    fn a_word_document_reads_as_markdown() {
        let body = [
            p(r#"<w:pStyle w:val="Ttulo"/>"#, &r("Informe")),
            p(r#"<w:pStyle w:val="Ttulo1"/>"#, &r("Parte")),
            p(r#"<w:pStyle w:val="Mio"/>"#, &r("Sub")),
            p(r#"<w:pStyle w:val="Ttulo4"/>"#, &r("Hondo")),
            p(
                "",
                &[
                    r("Un "),
                    r#"<w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">texto </w:t></w:r>"#.into(),
                    r#"<w:r><w:rPr><w:b/></w:rPr><w:t>fuerte</w:t></w:r>"#.into(),
                    r#"<w:r><w:rPr><w:i/><w:b w:val="0"/></w:rPr><w:t>,</w:t></w:r>"#.into(),
                    r#"<w:r><w:drawing><wp:inline><wp:docPr id="1" name="Imagen 1" descr="El logo"/></wp:inline></w:drawing></w:r>"#.into(),
                    r#"<w:r><w:br/><w:t>- a_b (c) # d</w:t></w:r>"#.into(),
                ]
                .concat(),
            ),
            p("", &r("1. no es lista")),
            "<w:p/>".into(),
            r#"<w:tbl>
                <w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr>
                <w:tr><w:tc><w:p><w:r><w:t>1|2</w:t></w:r></w:p></w:tc></w:tr>
            </w:tbl>"#
                .into(),
        ]
        .concat();
        let path = docx("a.docx", &body, &[]);
        assert_eq!(
            markdown(&path).unwrap(),
            "# Informe\n\n## Parte\n\n### Sub\n\n**Hondo**\n\n\
             Un **texto fuerte***,\\[Image: El logo\\]*  \n\u{200B}- a\\_b \\(c\\) # d\n\n\
             \u{200B}1. no es lista\n\n| A | B |\n|---|---|\n| 1\\|2 |  |\n\n"
        );
        let broken = zipped("broken.docx", &[("word/other.xml", "")]);
        assert!(markdown(&broken).is_err());
    }

    /// Bullets and numbers as the document's numbering says, from a paragraph or its style: a
    /// level under another starts again, a list continues after a paragraph, `numId` 0 is no
    /// list, and a list may start at another number.
    #[test]
    fn lists_keep_their_numbers() {
        let item = |list: &str, level: &str, text: &str| {
            p(
                &format!(
                    r#"<w:numPr><w:ilvl w:val="{level}"/><w:numId w:val="{list}"/></w:numPr>"#
                ),
                &r(text),
            )
        };
        let body = [
            item("2", "0", "uno"),
            item("2", "1", "a"),
            item("2", "1", "b"),
            item("2", "0", "dos"),
            item("2", "1", "otra a"),
            p("", &r("entre medio")),
            item("2", "0", "tres"),
            p(r#"<w:pStyle w:val="Vietas"/>"#, &r("punto")),
            p(
                r#"<w:pStyle w:val="Vietas"/><w:numPr><w:numId w:val="0"/></w:numPr>"#,
                &r("suelto"),
            ),
            item("3", "0", "siete"),
            item("9", "0", "sin lista"),
        ]
        .concat();
        let path = docx("lists.docx", &body, &[]);
        assert_eq!(
            markdown(&path).unwrap(),
            "1. uno\n  1. a\n  2. b\n2. dos\n  1. otra a\n\nentre medio\n\n3. tres\n- punto\n\n\
             suelto\n\n7. siete\n- sin lista\n"
        );
    }

    /// The default header and footer of the last section, around the body.
    #[test]
    fn the_header_and_footer_frame_the_body() {
        let edge = |tag: &str, text: &str| {
            format!(r#"<w:{tag} xmlns:w="{W}"><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:{tag}>"#)
        };
        let (header, footer) = (edge("hdr", "ACME"), edge("ftr", "Página 1"));
        let links = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
            <Relationship Id="rId8" Target="header1.xml"/><Relationship Id="rId9" Target="footer1.xml"/>
        </Relationships>"#;
        let body = format!(
            r#"{}<w:sectPr><w:headerReference w:type="first" r:id="rId9"/><w:headerReference w:type="default" r:id="rId8"/><w:footerReference w:type="default" r:id="rId9"/></w:sectPr>"#,
            p("", &r("Cuerpo"))
        );
        let path = docx(
            "edges.docx",
            &body,
            &[
                ("word/_rels/document.xml.rels", links),
                ("word/header1.xml", &header),
                ("word/footer1.xml", &footer),
            ],
        );
        assert_eq!(
            markdown(&path).unwrap(),
            "ACME\n\n---\n\nCuerpo\n\n---\n\nPágina 1\n\n"
        );
    }
}
