// Reads an EPUB into chapters of normalized words, using its table of contents.
use crate::matching::normalize;
use std::path::{Component, Path, PathBuf};

#[derive(Debug)]
pub struct Chapter {
    pub title: Option<String>, // None when the EPUB has no table of contents.
    pub words: Vec<String>,
}

// TOC entries shorter than this (e.g. a "Part Two" page) are merged into the next chapter.
const MIN_CHAPTER_WORDS: usize = 30;

pub fn load_chapters(path: &Path) -> Result<Vec<Chapter>, String> {
    let mut doc = epub::doc::EpubDoc::new(path)
        .map_err(|e| format!("Failed to open EPUB '{}': {}", path.display(), e))?;

    // Spine documents in reading order.
    let spine_paths: Vec<PathBuf> = doc
        .spine
        .iter()
        .filter_map(|item| doc.resources.get(&item.idref).map(|r| clean_path(&r.path)))
        .collect();
    let spine_html: Vec<String> = spine_paths
        .iter()
        .map(|p| doc.get_resource_str_by_path(p).unwrap_or_default())
        .collect();

    // TOC entries resolved to (spine index, byte offset) positions.
    let mut entries: Vec<(usize, usize, Option<String>)> = toc_entries(&mut doc)
        .into_iter()
        .filter_map(|(title, target)| {
            let (file, fragment) = target.split_once('#').map_or((target.as_str(), None), |(f, id)| (f, Some(id)));
            let index = spine_paths.iter().position(|p| *p == clean_path(Path::new(file)))?;
            let offset = fragment.map_or(Some(0), |id| anchor_offset(&spine_html[index], id))?;
            Some((index, offset, Some(title)))
        })
        .collect();
    if entries.is_empty() {
        // No usable TOC: one chapter per spine document, untitled.
        entries = (0..spine_html.len()).map(|i| (i, 0, None)).collect();
    }
    entries.sort_by_key(|&(index, offset, _)| (index, offset));

    let mut raw = Vec::new();
    for (i, (index, offset, title)) in entries.iter().enumerate() {
        let (end_index, end_offset) = entries
            .get(i + 1)
            .map_or((spine_html.len() - 1, usize::MAX), |&(ei, eo, _)| (ei, eo));
        let mut html = String::new();
        for doc_index in *index..=end_index {
            let page = &spine_html[doc_index];
            let from = if doc_index == *index { *offset } else { 0 };
            let to = if doc_index == end_index { end_offset.min(page.len()) } else { page.len() };
            html.push_str(page.get(from..to.max(from)).unwrap_or_default());
            html.push(' ');
        }
        raw.push((title.clone(), normalize(&html_to_text(&html))));
    }

    // Drop empty entries (cover images) and fold short ones into the next chapter.
    let mut chapters: Vec<Chapter> = Vec::new();
    let mut carry: Vec<String> = Vec::new();
    let count = raw.len();
    for (i, (title, words)) in raw.into_iter().enumerate() {
        carry.extend(words);
        if carry.is_empty() || (carry.len() < MIN_CHAPTER_WORDS && i + 1 < count) {
            continue;
        }
        chapters.push(Chapter { title, words: std::mem::take(&mut carry) });
    }
    if chapters.is_empty() {
        return Err("No chapter text found in the EPUB.".into());
    }
    Ok(chapters)
}

// (title, "path/in/zip.xhtml#fragment") from the EPUB2 NCX or the EPUB3 nav document.
fn toc_entries<R: std::io::Read + std::io::Seek>(doc: &mut epub::doc::EpubDoc<R>) -> Vec<(String, String)> {
    fn flatten(points: &[epub::doc::NavPoint], out: &mut Vec<(String, String)>) {
        for p in points {
            out.push((p.label.clone(), p.content.to_string_lossy().to_string()));
            flatten(&p.children, out);
        }
    }
    let mut out = Vec::new();
    flatten(&doc.toc, &mut out);
    if !out.is_empty() {
        return out;
    }

    // The crate only reads an NCX named by `<spine toc="...">`, which Calibre often omits.
    let ncx_path = doc.resources.values().find(|r| r.mime == "application/x-dtbncx+xml").map(|r| r.path.clone());
    if let Some(ncx_path) = ncx_path {
        let xml = doc.get_resource_str_by_path(&ncx_path).unwrap_or_default();
        let ncx_dir = ncx_path.parent().unwrap_or(Path::new(""));
        let point = regex::Regex::new(
            r#"(?s)<navLabel>\s*<text>(.*?)</text>\s*</navLabel>\s*<content\b[^>]*\bsrc\s*=\s*["']([^"']*)["']"#,
        )
        .unwrap();
        for cap in point.captures_iter(&xml) {
            let title = html_to_text(&cap[1]).split_whitespace().collect::<Vec<_>>().join(" ");
            out.push((title, ncx_dir.join(&cap[2]).to_string_lossy().to_string()));
        }
        if !out.is_empty() {
            return out;
        }
    }

    let Some(nav_path) = doc.get_nav_id().and_then(|id| doc.resources.get(&id).map(|r| r.path.clone())) else {
        return out;
    };
    let html = doc.get_resource_str_by_path(&nav_path).unwrap_or_default();
    let nav_dir = nav_path.parent().unwrap_or(Path::new(""));
    let toc_nav = regex::Regex::new(r#"(?s)<nav\b[^>]*epub:type\s*=\s*["'][^"']*\btoc\b[^"']*["'][^>]*>(.*?)</nav>"#).unwrap();
    let link = regex::Regex::new(r#"(?s)<a\b[^>]*\bhref\s*=\s*["']([^"']*)["'][^>]*>(.*?)</a>"#).unwrap();
    if let Some(nav) = toc_nav.captures(&html) {
        for cap in link.captures_iter(&nav[1]) {
            let title = html_to_text(&cap[2]).split_whitespace().collect::<Vec<_>>().join(" ");
            out.push((title, nav_dir.join(&cap[1]).to_string_lossy().to_string()));
        }
    }
    out
}

// Resolves "." and ".." so TOC links and spine paths compare equal.
// ponytail: no %-decoding of hrefs; add it if a book with spaces in file names shows up.
fn clean_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => { out.pop(); }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

// Byte offset of the tag carrying `id="<id>"`.
fn anchor_offset(html: &str, id: &str) -> Option<usize> {
    let at = html.find(&format!("id=\"{}\"", id)).or_else(|| html.find(&format!("id='{}'", id)))?;
    html[..at].rfind('<')
}

// Visible text of an (X)HTML fragment.
pub fn html_to_text(html: &str) -> String {
    let hidden = regex::Regex::new(r"(?is)<head\b.*?</head>|<script\b.*?</script>|<style\b.*?</style>").unwrap();
    // Inline tags join their text to neighbours (drop caps like <span>T</span>he); others separate words.
    let inline = regex::Regex::new(r"(?i)</?(a|b|i|em|strong|span|small|sup|sub|u|abbr)\b[^>]*>").unwrap();
    let tags = regex::Regex::new(r"(?s)<[^>]*>").unwrap();
    let entity = regex::Regex::new(r"&(#[xX][0-9a-fA-F]+|#\d+|[a-zA-Z]+);").unwrap();

    let text = hidden.replace_all(html, " ");
    let text = inline.replace_all(&text, "");
    let text = tags.replace_all(&text, " ");
    entity
        .replace_all(&text, |cap: &regex::Captures| {
            let name = &cap[1];
            let code = if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16).ok()
            } else if let Some(dec) = name.strip_prefix('#') {
                dec.parse().ok()
            } else {
                None
            };
            if let Some(c) = code.and_then(char::from_u32) {
                return c.to_string();
            }
            match name {
                "amp" => "&", "lt" => "<", "gt" => ">", "quot" => "\"", "apos" | "rsquo" | "lsquo" => "'",
                "mdash" | "ndash" => "-", _ => " ",
            }
            .to_string()
        })
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn para(words: &str, n: usize) -> String {
        (0..n).map(|i| format!("<p>{} {}.</p>", words, i)).collect()
    }

    fn page(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>Ignored head</title><style>p {{ color: red }}</style></head><body>{}</body></html>"#,
            body
        )
    }

    // Builds an EPUB with: cover (no text), ch1, a short "Part Two" page,
    // ch2 and ch3 sharing one file split by an anchor. `toc` is "ncx", "nav", or
    // "loose-ncx": an NCX the spine doesn't point to (as Calibre writes them).
    fn build_epub(name: &str, toc: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("abs-{}-{}.epub", name, std::process::id()));
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        let mut add = |name: &str, body: &str| {
            zip.start_file(name, opts).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        };

        add("mimetype", "application/epub+zip");
        add("META-INF/container.xml", r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#);
        let ncx = toc != "nav";
        let (version, toc_item, spine_attr) = if ncx {
            let attr = if toc == "ncx" { r#" toc="ncx""# } else { "" };
            ("2.0", r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#, attr)
        } else {
            ("3.0", r#"<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#, "")
        };
        add("OEBPS/content.opf", &format!(
            r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="{version}" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>T</dc:title></metadata><manifest>{toc_item}<item id="cover" href="Text/cover.xhtml" media-type="application/xhtml+xml"/><item id="c1" href="Text/ch1.xhtml" media-type="application/xhtml+xml"/><item id="p2" href="Text/part2.xhtml" media-type="application/xhtml+xml"/><item id="c23" href="Text/ch23.xhtml" media-type="application/xhtml+xml"/></manifest><spine{spine_attr}><itemref idref="cover"/><itemref idref="c1"/><itemref idref="p2"/><itemref idref="c23"/></spine></package>"#
        ));
        add("OEBPS/Text/cover.xhtml", &page(r#"<img src="cover.jpg"/>"#));
        add("OEBPS/Text/ch1.xhtml", &page(&format!("<h1>Chapter 1</h1>{}", para("Alpha &amp; words", 12))));
        add("OEBPS/Text/part2.xhtml", &page("<h1>Part II</h1>"));
        add("OEBPS/Text/ch23.xhtml", &page(&format!(
            r#"<h1>Chapter 2</h1>{}<h1 id="c3">Chapter 3</h1>{}"#,
            para("Bravo&#8217;s words", 12),
            para("Charlie&nbsp;words", 12)
        )));

        let entries = [
            ("Cover", "Text/cover.xhtml"),
            ("Chapter One", "Text/ch1.xhtml"),
            ("Part Two", "Text/part2.xhtml"),
            ("Chapter Two", "Text/ch23.xhtml"),
            ("Chapter Three", "Text/ch23.xhtml#c3"),
        ];
        if ncx {
            let points: String = entries.iter().enumerate().map(|(i, (label, src))| format!(
                r#"<navPoint id="n{i}" playOrder="{}"><navLabel><text>{label}</text></navLabel><content src="{src}"/></navPoint>"#, i + 1
            )).collect();
            add("OEBPS/toc.ncx", &format!(r#"<?xml version="1.0"?><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1"><docTitle><text>T</text></docTitle><navMap>{points}</navMap></ncx>"#));
        } else {
            // Nested list, as real nav documents often have parts containing chapters.
            let li = |(label, src): &(&str, &str)| format!(r#"<li><a href="{src}"><span>{label}</span></a></li>"#);
            let body = format!(
                r#"<nav epub:type="landmarks"><ol><li><a href="Text/ch1.xhtml">Start</a></li></ol></nav><nav epub:type="toc"><ol>{}{}<li><a href="Text/part2.xhtml">Part Two</a><ol>{}{}</ol></li></ol></nav>"#,
                li(&entries[0]), li(&entries[1]), li(&entries[3]), li(&entries[4])
            );
            add("OEBPS/nav.xhtml", &page(&body).replace("<html ", r#"<html xmlns:epub="http://www.idpf.org/2007/ops" "#));
        }
        zip.finish().unwrap();
        path
    }

    fn check(chapters: &[Chapter]) {
        let titles: Vec<_> = chapters.iter().map(|c| c.title.as_deref().unwrap()).collect();
        // Cover has no text (dropped); "Part Two" is merged into Chapter Two.
        assert_eq!(titles, ["Chapter One", "Chapter Two", "Chapter Three"]);
        assert_eq!(chapters[0].words[..4], ["chapter", "one", "alpha", "words"]);
        assert_eq!(chapters[1].words[..6], ["part", "two", "chapter", "two", "bravos", "words"]);
        assert_eq!(chapters[2].words[..4], ["chapter", "three", "charlie", "words"]);
        assert_eq!(chapters[1].words.last().unwrap(), "eleven");
        assert!(!chapters.iter().any(|c| c.words.contains(&"ignored".to_string())));
    }

    #[test]
    fn loads_epub2_ncx_toc() {
        check(&load_chapters(&build_epub("ncx", "ncx")).unwrap());
    }

    #[test]
    fn loads_ncx_the_spine_does_not_reference() {
        check(&load_chapters(&build_epub("loose-ncx", "loose-ncx")).unwrap());
    }

    #[test]
    fn loads_epub3_nav_toc() {
        check(&load_chapters(&build_epub("nav", "nav")).unwrap());
    }

    #[test]
    fn html_to_text_strips_tags_and_entities() {
        assert_eq!(
            html_to_text("<head><title>x</title></head><p>A&amp;B&nbsp;<i>c</i>&#8217;s &#x41;</p><script>var y;</script>").split_whitespace().collect::<Vec<_>>(),
            ["A&B", "c\u{2019}s", "A"]
        );
    }

    #[test]
    fn missing_file_is_an_error() {
        assert!(load_chapters(Path::new("/nope.epub")).is_err());
    }
}
