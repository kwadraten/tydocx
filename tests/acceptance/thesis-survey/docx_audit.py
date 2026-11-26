"""Namespace-aware OPC and WordprocessingML evidence, without updating fields."""

from collections import Counter
import posixpath
import re
from urllib.parse import unquote, urlsplit
from zipfile import ZipFile

from lxml import etree as ET

W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
NS = {"w": W, "r": R, "wp": "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
      "a": "http://schemas.openxmlformats.org/drawingml/2006/main",
      "m": "http://schemas.openxmlformats.org/officeDocument/2006/math"}
TEXT = "w:t|m:t|w:tab|w:br"


def local(name):
    return name.split("}")[-1]


def attrs(node):
    return {local(k): v for k, v in node.attrib.items()} if node is not None else {}


def xpath(node, query):
    return node.xpath(query, namespaces=NS)


def props(node, path):
    return {local(el.tag): attrs(el) for el in xpath(node, path)}


def text(el):
    return "".join(
        item.text or "" if local(item.tag) == "t" else "\t" if local(item.tag) == "tab" else "\n"
        for item in xpath(el, ".//w:t|.//m:t|.//w:tab|.//w:br")
    )


def docx_audit(path):
    parse_errors, relationship_issues = [], []
    with ZipFile(path) as archive:
        corrupt = archive.testzip()
        names = [n for n in archive.namelist() if not n.endswith("/")]
        duplicates = [x for x, c in Counter(names).items() if c > 1]
        parts = {name: None for name in names}
        for name in names:
            if name.endswith((".xml", ".rels", ".svg")):
                try:
                    parts[name] = ET.fromstring(archive.read(name), ET.XMLParser(
                        resolve_entities=False, no_network=True, load_dtd=False))
                except ET.XMLSyntaxError as exc:
                    parse_errors.append([name, str(exc)])
    essential = ["[Content_Types].xml", "_rels/.rels", "word/document.xml"]
    missing_essential = [part for part in essential if parts.get(part) is None]
    content_types = parts.get("[Content_Types].xml")
    if content_types is not None:
        extensions = {el.get("Extension") for el in content_types if local(el.tag) == "Default"}
        overrides = {el.get("PartName", "")[1:] for el in content_types if local(el.tag) == "Override"}
        untyped = [n for n in names if n not in overrides and n != essential[0]
                   and n.rsplit(".", 1)[-1] not in extensions]
    else:
        untyped = names
    relationships = {}
    for name, root in parts.items():
        if not name.endswith(".rels") or root is None:
            continue
        folder, tail = posixpath.split(name)
        owner = "" if name == "_rels/.rels" else posixpath.join(posixpath.dirname(folder), tail[:-5])
        if owner and owner not in parts:
            relationship_issues.append([name, "missing relationship owner", owner])
        rels = relationships.setdefault(owner, {})
        for el in root:
            at = attrs(el)
            rid = at["Id"]
            target = at.get("Target", "")
            if rid in rels:
                relationship_issues.append([name, "duplicate Id", rid])
            if at.get("TargetMode") != "External":
                raw = unquote(urlsplit(target).path)
                resolved = posixpath.normpath(raw[1:] if raw.startswith("/") else
                                              posixpath.join(posixpath.dirname(owner), raw))
                at["resolved"] = resolved
                if resolved not in parts:
                    relationship_issues.append([name, "missing target", rid, resolved])
            rels[rid] = at
    for name, root in parts.items():
        if root is None or not name.endswith(".xml"):
            continue
        for el in root.iter():
            for attr, val in el.attrib.items():
                if attr in {f"{{{R}}}{x}" for x in ("id", "embed", "link")} and val not in relationships.get(name, {}):
                    relationship_issues.append([name, "unresolved r attribute", local(el.tag), val])
    doc = parts.get("word/document.xml")
    counts = Counter()
    fields, paragraphs, bookmarks, sections, furniture, drawings = [], [], [], [], [], []
    mark_issues, unknown_targets = [], []
    if doc is not None:
        tree = doc.getroottree()
        counts.update(local(e.tag) for e in doc.iter())
        for node in xpath(doc, "//w:sectPr"):
            sections.append({"path": tree.getpath(node), "attributes": [dict(tag=local(e.tag), **attrs(e)) for e in node]})
        for node in xpath(doc, "//w:p"):
            if xpath(node, "ancestor::w:r"):
                # Textbox paragraphs are audited independently; avoid double text in outer paragraphs.
                continue
            ptext = text(node)
            if not ptext and not xpath(node, ".//w:drawing|.//w:pPr/w:sectPr|.//w:br"):
                continue
            paragraphs.append({"path": tree.getpath(node), "text": ptext[:2400],
                               "pPr": props(node, "./w:pPr/*"),
                               "runs": [{"text": text(run)[:400], "rPr": props(run, "./w:rPr/*")}
                                        for run in xpath(node, "./w:r|./w:hyperlink/w:r") if text(run)],
                               "tabStops": [attrs(t) for t in xpath(node, "./w:pPr/w:tabs/w:tab")]})
        for node in xpath(doc, "//wp:inline|//wp:anchor"):
            drawings.append({"path": tree.getpath(node), "type": local(node.tag), "attributes": attrs(node),
                             "extent": [attrs(x) for x in xpath(node, "./wp:extent")],
                             "alt": [attrs(x) for x in xpath(node, "./wp:docPr")],
                             "crop": [attrs(x) for x in xpath(node, ".//a:srcRect")],
                             "image": [attrs(x) for x in xpath(node, ".//a:blip")]})
        names_in_doc = Counter(node.get(f"{{{W}}}name") for node in xpath(doc, "//w:bookmarkStart"))
        begins = Counter(node.get(f"{{{W}}}id") for node in xpath(doc, "//w:bookmarkStart"))
        ends = Counter(node.get(f"{{{W}}}id") for node in xpath(doc, "//w:bookmarkEnd"))
        mark_issues = [[reason, key, val] for reason, counter in
                       (("duplicate name", names_in_doc), ("duplicate start id", begins), ("duplicate end id", ends))
                       for key, val in counter.items() if val > 1]
        mark_issues += [["unbalanced bookmark id", key, begins[key], ends[key]]
                        for key in begins.keys() | ends.keys() if begins[key] != ends[key]]
        bookmarks = [dict(path=tree.getpath(node), **attrs(node)) for node in xpath(doc, "//w:bookmarkStart")]
        for node in xpath(doc, "//w:hyperlink[@w:anchor]"):
            target = node.get(f"{{{W}}}anchor")
            if target not in names_in_doc:
                unknown_targets.append(["hyperlink", tree.getpath(node), target, text(node)])
        # Gather split instrText across runs; do not require the editor to refresh fields.
        stack = []
        for node in xpath(doc, "//w:fldSimple|//w:fldChar|//w:instrText"):
            kind = local(node.tag)
            if kind == "fldSimple":
                fields.append([tree.getpath(node), node.get(f"{{{W}}}instr"), "simple", text(node)])
            elif kind == "fldChar":
                event = node.get(f"{{{W}}}fldCharType")
                if event == "begin":
                    stack.append([tree.getpath(node), "", "open", ""])
                elif event == "separate" and stack:
                    stack[-1][2] = "cached"
                elif event == "end" and stack:
                    fields.append(stack.pop())
                elif event in {"end", "separate"}:
                    mark_issues.append(["unmatched field marker", tree.getpath(node), event])
            elif stack and stack[-1][2] == "open":
                stack[-1][1] += node.text or ""
        for field in stack:
            mark_issues.append(["unclosed field", field])
        for origin, instr, _, _ in fields:
            match = re.match(r'\s*(REF|PAGEREF|NOTEREF)\s+(?:"([^"]+)"|(\S+))', instr or "", re.I)
            if match and (target := (match[2] or match[3])) not in names_in_doc:
                unknown_targets.append([match[1], origin, target])
    for name, part in parts.items():
        if part is not None and re.fullmatch(r"word/(header\d*|footer\d*|footnotes|endnotes)\.xml", name):
            furniture.append({"part": name, "paragraphs": [text(p) for p in xpath(part, "//w:p")],
                              "fields": [n.text for n in xpath(part, "//w:instrText")]})
    styles = parts.get("word/styles.xml")
    styles_audit = {} if styles is None else {
        "default_run": props(styles, "./w:docDefaults/w:rPrDefault/w:rPr/*"),
        "default_paragraph": props(styles, "./w:docDefaults/w:pPrDefault/w:pPr/*"),
        "styles": [{"style": attrs(el), "pPr": props(el, "./w:pPr/*"), "rPr": props(el, "./w:rPr/*")}
                   for el in xpath(styles, "./w:style")],
    }
    return {
        "zip_bad_crc": corrupt, "zip_duplicate_names": duplicates, "missing_essential": missing_essential,
        "xml_parse_errors": parse_errors, "untyped_parts": untyped, "relationship_issues": relationship_issues,
        "relationships": relationships, "parts": names, "document_element_counts": counts,
        "bookmarks": bookmarks, "field_or_bookmark_issues": mark_issues, "fields": fields,
        "unresolved_navigation_targets": unknown_targets, "sections": sections, "drawings": drawings,
        "paragraphs": paragraphs, "furniture": furniture, "styles": styles_audit,
    }
