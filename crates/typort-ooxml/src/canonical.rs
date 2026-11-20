//! Canonical forms over generated OOXML parts, used by the conformance suite
//! to compare documents without being fooled by serialization noise:
//! attribute order, element-id churn, and self-closing versus paired tags.
//!
//! Element and attribute names keep their qualified (`prefix:local`) form.
//! That is namespace-aware for this backend's purposes: the writer binds each
//! prefix to a fixed URI, so equal qualified names are equal expanded names
//! and different qualified names are different expanded names.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io;

use quick_xml::Reader;
use quick_xml::events::Event;

/// True for attributes that identify an instance rather than carry meaning:
/// bookmark/comment `w:id`, drawing `id`, Word's paragraph and rsid
/// bookkeeping. References (`w:numId`, `r:embed`, `w:anchor`) are kept.
fn is_volatile_attribute(qualified_name: &str) -> bool {
    let local = qualified_name.rsplit(':').next().unwrap_or(qualified_name);
    local == "id" || local == "paraId" || local == "textId" || local.starts_with("rsid")
}

fn sorted_attributes(
    element: &quick_xml::events::BytesStart<'_>,
    decoder: quick_xml::Decoder,
) -> io::Result<Vec<(String, String)>> {
    let mut attrs = Vec::new();
    for attr in element.attributes() {
        let attr = attr.map_err(io::Error::other)?;
        let name = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
        if is_volatile_attribute(&name) {
            continue;
        }
        let value = attr
            .decode_and_unescape_value(decoder)
            .map_err(io::Error::other)?
            .into_owned();
        attrs.push((name, value));
    }
    attrs.sort();
    Ok(attrs)
}

fn qualified_name(name: quick_xml::name::QName<'_>) -> String {
    String::from_utf8_lossy(name.as_ref()).into_owned()
}

fn push_attrs(out: &mut String, attrs: &[(String, String)]) {
    for (name, value) in attrs {
        let _ = write!(out, " {name}={value:?}");
    }
}

fn normalize_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Canonical fingerprint of an XML part: one indented line per event, sorted
/// attributes, volatile ids dropped, text whitespace-normalized.
///
/// # Errors
/// Returns an error if `xml` is not well-formed XML.
pub fn canonical_fingerprint(xml: &str) -> io::Result<String> {
    canonical(xml, true)
}

/// Semantic digest of an XML part: the fingerprint's element-class and text
/// skeleton with all attributes removed.
///
/// # Errors
/// Returns an error if `xml` is not well-formed XML.
pub fn semantic_digest(xml: &str) -> io::Result<String> {
    canonical(xml, false)
}

fn canonical(xml: &str, with_attributes: bool) -> io::Result<String> {
    let mut reader = Reader::from_str(xml);
    let decoder = reader.decoder();
    let mut out = String::new();
    let mut depth = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = qualified_name(e.name());
                out.push_str(&"  ".repeat(depth));
                out.push_str(&name);
                if with_attributes {
                    push_attrs(&mut out, &sorted_attributes(&e, decoder)?);
                }
                out.push('\n');
                depth += 1;
            }
            Ok(Event::End(e)) => {
                depth = depth.saturating_sub(1);
                out.push_str(&"  ".repeat(depth));
                out.push('/');
                out.push_str(&qualified_name(e.name()));
                out.push('\n');
            }
            Ok(Event::Empty(e)) => {
                let name = qualified_name(e.name());
                out.push_str(&"  ".repeat(depth));
                out.push_str(&name);
                if with_attributes {
                    push_attrs(&mut out, &sorted_attributes(&e, decoder)?);
                }
                out.push_str("/\n");
            }
            Ok(Event::Text(e)) => {
                let text = normalize_text(&e.unescape().map_err(io::Error::other)?);
                if !text.is_empty() {
                    out.push_str(&"  ".repeat(depth));
                    let _ = writeln!(out, "={text:?}");
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(io::Error::other(e)),
        }
    }
    Ok(out)
}

/// The set of distinct qualified element names present in a part.
///
/// # Errors
/// Returns an error if `xml` is not well-formed XML.
pub fn element_classes(xml: &str) -> io::Result<BTreeSet<String>> {
    let mut reader = Reader::from_str(xml);
    let mut classes = BTreeSet::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                classes.insert(qualified_name(e.name()));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(io::Error::other(e)),
        }
    }
    Ok(classes)
}

/// The concatenated `w:t` text of every `w:p` in a part, in document order.
/// Field instructions (`w:instrText`) and math (`m:t`) are element content
/// too, but they are field payloads and math runs — not paragraph text.
///
/// # Errors
/// Returns an error if `xml` is not well-formed XML.
pub fn paragraph_texts(xml: &str) -> io::Result<Vec<String>> {
    let mut reader = Reader::from_str(xml);
    let mut paragraphs = Vec::new();
    let mut current: Option<String> = None;
    let mut stack: Vec<Vec<u8>> = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name().as_ref().to_vec();
                if name == b"w:p" {
                    current = Some(String::new());
                }
                stack.push(name);
            }
            Ok(Event::Empty(e)) => {
                if e.name().as_ref() == b"w:p" {
                    paragraphs.push(String::new());
                }
            }
            Ok(Event::End(e)) => {
                stack.pop();
                if e.name().as_ref() == b"w:p"
                    && let Some(text) = current.take()
                {
                    paragraphs.push(text);
                }
            }
            Ok(Event::Text(e)) => {
                if stack.last().is_some_and(|name| name == b"w:t")
                    && let Some(text) = current.as_mut()
                {
                    text.push_str(&e.unescape().map_err(io::Error::other)?);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(io::Error::other(e)),
        }
    }
    Ok(paragraphs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_sorts_attributes() {
        let a = canonical_fingerprint("<w:p><w:r a=\"1\" b=\"2\"/><w:r b=\"2\" a=\"1\"/></w:p>")
            .unwrap();
        assert_eq!(
            a.matches("w:r").count(),
            2,
            "both runs fingerprint identically"
        );
        let first = a.find("w:r a=\"1\" b=\"2\"").unwrap();
        let second = a.rfind("w:r a=\"1\" b=\"2\"").unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn fingerprint_ignores_ids() {
        let a =
            canonical_fingerprint("<w:p w14:paraId=\"AAA\" w:id=\"1\"><w:t>x</w:t></w:p>").unwrap();
        let b =
            canonical_fingerprint("<w:p w14:paraId=\"BBB\" w:id=\"2\"><w:t>x</w:t></w:p>").unwrap();
        assert_eq!(a, b);
        assert!(!a.contains("paraId"));
    }

    #[test]
    fn fingerprint_keeps_references() {
        let fp = canonical_fingerprint("<w:numPr><w:numId w:val=\"7\"/></w:numPr>").unwrap();
        assert!(
            fp.contains("numId"),
            "numId is a reference, not an instance id"
        );
    }

    #[test]
    fn digest_drops_attributes_keeps_text() {
        let digest = semantic_digest("<w:p a=\"1\"><w:r><w:t>hello</w:t></w:r></w:p>").unwrap();
        assert!(!digest.contains("a="));
        assert!(digest.contains("hello"));
    }

    #[test]
    fn paragraph_texts_concatenates_runs() {
        let ps = paragraph_texts(
            "<w:body><w:p><w:r><w:t>a</w:t></w:r><w:r><w:t>b</w:t></w:r></w:p><w:p/></w:body>",
        )
        .unwrap();
        assert_eq!(ps, vec!["ab".to_owned(), String::new()]);
    }
}
