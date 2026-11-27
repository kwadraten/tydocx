use std::cell::RefCell;
use std::io::{self, Write};
use std::net::Ipv6Addr;

use quick_xml::Writer;

use super::WriteCtx;
use super::run::{write_inline, write_run};
use crate::ooxml::document::InlineElement;

#[derive(Default)]
pub(super) struct HyperlinkRelationships(RefCell<Vec<String>>);

impl HyperlinkRelationships {
    pub(super) fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }

    fn add(&self, target: String) -> String {
        let mut targets = self.0.borrow_mut();
        targets.push(target);
        hyperlink_id(targets.len())
    }

    pub(super) fn write<W: Write>(&self, writer: &mut Writer<W>) -> io::Result<()> {
        for (index, target) in self.0.borrow().iter().enumerate() {
            writer
                .create_element("Relationship")
                .with_attribute(("Id", hyperlink_id(index + 1).as_str()))
                .with_attribute((
                    "Type",
                    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink",
                ))
                .with_attribute(("Target", target.as_str()))
                .with_attribute(("TargetMode", "External"))
                .write_empty()?;
        }
        Ok(())
    }
}

fn hyperlink_id(index: usize) -> String {
    format!("rIdHyperlink{index}")
}

pub(super) fn write_hyperlink<W: Write>(
    writer: &mut Writer<W>,
    url: &str,
    body: &[InlineElement],
    preserve_style: bool,
    ctx: &WriteCtx,
) -> io::Result<()> {
    let target = encoded_target(url).or_else(|| preserve_style.then(|| url.to_owned()));
    let (tag, attribute, value) = if let Some(target) = target {
        ("w:hyperlink", "r:id", ctx.hyperlinks.add(target))
    } else {
        ("w:fldSimple", "w:instr", format!(r#"HYPERLINK "{url}""#))
    };

    writer
        .create_element(tag)
        .with_attribute((attribute, value.as_str()))
        .write_inner_content(|w| {
            for inline in body {
                if !preserve_style && let InlineElement::Text(run) = inline {
                    let mut styled = run.clone();
                    styled.color = Some(ctx.doc_style.hyperlink_color.clone());
                    styled.underline = true;
                    write_run(w, &styled, ctx.doc_style)?;
                } else {
                    write_inline(w, inline, ctx)?;
                }
            }
            Ok(())
        })?;
    Ok(())
}

fn encoded_target(url: &str) -> Option<String> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let brackets = ipv6_brackets(url);
    let keep = |index, byte| match byte {
        b'%' => has_hex_escape(url.as_bytes(), index),
        b'[' | b']' => brackets.is_some_and(|(left, right)| index == left || index == right),
        _ => is_unreserved(byte) || b":/?#@!$&'()*+,;=".contains(&byte),
    };
    if url
        .bytes()
        .enumerate()
        .all(|(index, byte)| keep(index, byte))
    {
        return None;
    }
    let mut encoded = String::with_capacity(url.len());
    for (index, byte) in url.bytes().enumerate() {
        if keep(index, byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    Some(encoded)
}

fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"-._~".contains(&byte)
}

fn has_hex_escape(bytes: &[u8], index: usize) -> bool {
    bytes
        .get(index + 1..index + 3)
        .is_some_and(|digits| digits.iter().all(u8::is_ascii_hexdigit))
}

fn ipv6_brackets(url: &str) -> Option<(usize, usize)> {
    let authority_start = if url.starts_with("//") {
        2
    } else {
        let (scheme, _) = url.split_once("://")?;
        let (first, rest) = scheme.as_bytes().split_first()?;
        if !first.is_ascii_alphabetic()
            || !rest
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || b"+.-".contains(byte))
        {
            return None;
        }
        scheme.len() + 3
    };
    let authority = url[authority_start..].split(['/', '?', '#']).next()?;
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let (literal, port) = host_port.strip_prefix('[')?.split_once(']')?;
    if !port.is_empty()
        && !port
            .strip_prefix(':')
            .is_some_and(|digits| digits.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let address = if let Some((address, zone)) = literal.split_once("%25") {
        if zone.is_empty()
            || !zone.bytes().enumerate().all(|(i, byte)| {
                is_unreserved(byte) || (byte == b'%' && has_hex_escape(zone.as_bytes(), i))
            })
        {
            return None;
        }
        address
    } else {
        literal
    };
    address.parse::<Ipv6Addr>().ok()?;
    let left = authority_start + authority.len() - host_port.len();
    Some((left, left + 1 + literal.len()))
}
