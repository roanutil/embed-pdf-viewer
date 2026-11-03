//! A small XML reader that keeps where everything is in the text, so a
//! caller can change one element and leave every other character as it was
//! written. Ports `services/src/shared/xml.ts`.
//!
//! It reads elements, attributes, text and CDATA, resolves namespaces, and
//! decodes the predefined entities and character references. It skips the
//! XML declaration, processing instructions, comments and a DOCTYPE. Text
//! that isn't well-formed (a tag that doesn't close, an unknown entity, a
//! stray `<`) is unreadable: [`parse_xml`] answers `None` rather than guess.
//!
//! Positions are byte offsets into the text. Upstream counts UTF-16 units;
//! every delimiter the reader looks for is ASCII, so both land on the same
//! characters, and a splice made at one is the splice made at the other.

/// The namespace the `xml` prefix is always bound to (`xml:lang`).
pub const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";
/// The namespace of `xmlns` declarations.
pub const XMLNS_NAMESPACE: &str = "http://www.w3.org/2000/xmlns/";

/// Prefix to namespace, in the order they were first bound (a `Map`'s
/// order upstream): `""` is the default namespace.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scope(Vec<(String, String)>);

impl Scope {
    fn set(&mut self, prefix: &str, ns: &str) {
        match self.0.iter_mut().find(|(bound, _)| bound == prefix) {
            Some(entry) => entry.1 = ns.to_owned(),
            None => self.0.push((prefix.to_owned(), ns.to_owned())),
        }
    }

    pub fn get(&self, prefix: &str) -> Option<&str> {
        self.0.iter().find(|(bound, _)| bound == prefix).map(|(_, ns)| ns.as_str())
    }

    pub fn has(&self, prefix: &str) -> bool {
        self.get(prefix).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(prefix, ns)| (prefix.as_str(), ns.as_str()))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct XmlAttribute {
    /// The name as written, such as `rdf:about`.
    pub qname: String,
    pub prefix: String,
    pub local: String,
    /// The namespace its prefix is bound to; `None` for an unprefixed
    /// attribute (attributes take no default namespace) or an unbound
    /// prefix.
    pub ns: Option<String>,
    /// The value, decoded.
    pub value: String,
    /// Where its name starts, and just after its closing quote.
    pub start: usize,
    pub end: usize,
    /// Where its value starts and ends, between the quotes.
    pub value_start: usize,
    pub value_end: usize,
}

/// One element. Its children are indices into [`XmlDocument::elements`].
#[derive(Debug, Clone, PartialEq)]
pub struct XmlElement {
    /// The name as written, such as `dc:title`.
    pub qname: String,
    pub prefix: String,
    pub local: String,
    /// The namespace of its name; `None` when it has none or the prefix is
    /// unbound.
    pub ns: Option<String>,
    pub attributes: Vec<XmlAttribute>,
    /// Prefix to namespace in scope here, its own declarations included.
    pub scope: Scope,
    pub children: Vec<usize>,
    /// The text and CDATA directly inside it, decoded.
    pub text: String,
    /// The `<` of its start tag.
    pub start: usize,
    /// Where its start tag's `/>` or `>` begins.
    pub start_tag_close: usize,
    /// Just after its start tag.
    pub start_tag_end: usize,
    /// Whether its start tag closes it (`<a/>`).
    pub self_closing: bool,
    /// The `<` of its end tag; `start_tag_end` when it closes itself.
    pub end_tag_start: usize,
    /// Just after its end tag (or its start tag, when it closes itself).
    pub end: usize,
}

/// The text's elements, in document order: the root is the first.
#[derive(Debug, Clone, PartialEq)]
pub struct XmlDocument {
    pub elements: Vec<XmlElement>,
}

impl XmlDocument {
    pub fn element(&self, index: usize) -> &XmlElement {
        &self.elements[index]
    }
}

/// The text's elements, from its one root; `None` when it isn't
/// well-formed. Elements are stored in document order (`walkXml`'s), the
/// root at 0.
pub fn parse_xml(text: &str) -> Option<XmlDocument> {
    let mut reader =
        Reader { text, bytes: text.as_bytes(), at: 0, open: Vec::new(), elements: Vec::new() };
    reader.read().ok()?;
    Some(XmlDocument { elements: reader.elements })
}

/// `value` as XML text: `&`, `<` and `>` escaped, characters XML can't hold
/// dropped.
pub fn escape_xml_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars().filter(|c| writable(*c)) {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#xD;"),
            c => out.push(c),
        }
    }
    out
}

/// `value` as an attribute value between either kind of quote.
pub fn escape_xml_attribute(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in escape_xml_text(value).chars() {
        match c {
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' => out.push_str("&#x9;"),
            '\n' => out.push_str("&#xA;"),
            c => out.push(c),
        }
    }
    out
}

/// `prefix:local`, or `local` alone for no prefix.
pub fn xml_name(prefix: &str, local: &str) -> String {
    if prefix.is_empty() { local.to_owned() } else { format!("{prefix}:{local}") }
}

/// Whether XML 1.0 can hold `c` at all: not a control other than tab, line
/// feed and carriage return, nor `U+FFFE` or `U+FFFF`. (A lone surrogate,
/// which upstream drops too, is not a Rust `char`.)
fn writable(c: char) -> bool {
    let code = u32::from(c);
    let control = code < 0x20 && code != 0x09 && code != 0x0a && code != 0x0d;
    !control && code != 0xfffe && code != 0xffff
}

/// JavaScript's `\s`.
pub fn js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

struct Unreadable;

fn space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

fn name_end(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'>' | b'=' | b'<' | b'"' | b'\'')
}

/// The predefined entities and character references decoded; unreadable for
/// any other `&`.
fn decode(raw: &str) -> Result<String, Unreadable> {
    if !raw.contains('&') {
        return Ok(raw.to_owned());
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let semi = after.find(';').ok_or(Unreadable)?;
        let body = &after[..semi];
        match body {
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "amp" => out.push('&'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ => {
                let code = if let Some(hex) = body.strip_prefix("#x") {
                    if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                        return Err(Unreadable);
                    }
                    u32::from_str_radix(hex, 16).unwrap_or(u32::MAX)
                } else if let Some(decimal) = body.strip_prefix('#') {
                    if decimal.is_empty() || !decimal.bytes().all(|b| b.is_ascii_digit()) {
                        return Err(Unreadable);
                    }
                    decimal.parse::<u32>().unwrap_or(u32::MAX)
                } else {
                    return Err(Unreadable);
                };
                if code > 0x10ffff {
                    return Err(Unreadable);
                }
                // A surrogate's code makes a lone surrogate upstream, which a
                // Rust string can't hold.
                out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
            }
        }
        rest = &after[semi + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn split_name(qname: &str) -> (String, String) {
    match qname.find(':') {
        None => (String::new(), qname.to_owned()),
        Some(colon) => (qname[..colon].to_owned(), qname[colon + 1..].to_owned()),
    }
}

struct Reader<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
    open: Vec<usize>,
    elements: Vec<XmlElement>,
}

impl<'a> Reader<'a> {
    fn read(&mut self) -> Result<(), Unreadable> {
        let text = self.text;
        while self.at < text.len() {
            let lt = text[self.at..].find('<').map(|found| self.at + found);
            let text_end = lt.unwrap_or(text.len());
            if text_end > self.at {
                self.add_text(&text[self.at..text_end], false)?;
            }
            let Some(lt) = lt else { break };
            let rest = &text[lt..];
            if rest.starts_with("<!--") {
                self.at = self.after("-->", lt + 4)?;
            } else if rest.starts_with("<![CDATA[") {
                self.read_cdata(lt)?;
            } else if rest.starts_with("<?") {
                self.at = self.after("?>", lt + 2)?;
            } else if rest.starts_with("<!") {
                self.skip_declaration(lt)?;
            } else if rest.starts_with("</") {
                self.read_end_tag(lt)?;
            } else {
                self.read_start_tag(lt)?;
            }
        }
        if !self.open.is_empty() || self.elements.is_empty() {
            return Err(Unreadable);
        }
        Ok(())
    }

    /// Just after the next `token` from `from`.
    fn after(&self, token: &str, from: usize) -> Result<usize, Unreadable> {
        let found = self.text.get(from..).and_then(|rest| rest.find(token)).ok_or(Unreadable)?;
        Ok(from + found + token.len())
    }

    fn skip_space(&self, from: usize) -> usize {
        let mut i = from;
        while i < self.bytes.len() && space(self.bytes[i]) {
            i += 1;
        }
        i
    }

    fn read_name(&self, from: usize) -> Result<&'a str, Unreadable> {
        let mut i = from;
        while i < self.bytes.len() && !name_end(self.bytes[i]) {
            i += 1;
        }
        if i == from {
            return Err(Unreadable);
        }
        Ok(&self.text[from..i])
    }

    /// Text inside the open element; text outside the root (packet padding)
    /// is passed over.
    fn add_text(&mut self, raw: &str, cdata: bool) -> Result<(), Unreadable> {
        let Some(&current) = self.open.last() else {
            return Ok(());
        };
        let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
        let decoded = if cdata { normalized } else { decode(&normalized)? };
        self.elements[current].text.push_str(&decoded);
        Ok(())
    }

    fn read_cdata(&mut self, lt: usize) -> Result<(), Unreadable> {
        let close = self.text[lt + 9..].find("]]>").map(|at| lt + 9 + at).ok_or(Unreadable)?;
        let text = self.text;
        self.add_text(&text[lt + 9..close], true)?;
        self.at = close + 3;
        Ok(())
    }

    /// `<!DOCTYPE …>`, with an internal subset in brackets.
    fn skip_declaration(&mut self, lt: usize) -> Result<(), Unreadable> {
        let mut depth = 0i64;
        for i in lt + 2..self.bytes.len() {
            match self.bytes[i] {
                b'[' => depth += 1,
                b']' => depth -= 1,
                b'>' if depth <= 0 => {
                    self.at = i + 1;
                    return Ok(());
                }
                _ => {}
            }
        }
        Err(Unreadable)
    }

    fn read_start_tag(&mut self, lt: usize) -> Result<(), Unreadable> {
        let text = self.text;
        let bytes = self.bytes;
        let qname = self.read_name(lt + 1)?.to_owned();
        let mut attributes: Vec<XmlAttribute> = Vec::new();
        let mut i = lt + 1 + qname.len();
        let start_tag_close;
        let mut self_closing = false;
        loop {
            let next = self.skip_space(i);
            if text[next..].starts_with("/>") {
                start_tag_close = next;
                self_closing = true;
                break;
            }
            if bytes.get(next) == Some(&b'>') {
                start_tag_close = next;
                break;
            }
            // An attribute needs white space before it.
            if next == i || next >= bytes.len() {
                return Err(Unreadable);
            }
            let name = self.read_name(next)?.to_owned();
            let mut j = self.skip_space(next + name.len());
            if bytes.get(j) != Some(&b'=') {
                return Err(Unreadable);
            }
            j = self.skip_space(j + 1);
            let quote = match bytes.get(j) {
                Some(&q) if q == b'"' || q == b'\'' => q as char,
                _ => return Err(Unreadable),
            };
            let close = text[j + 1..].find(quote).map(|at| j + 1 + at).ok_or(Unreadable)?;
            let raw = &text[j + 1..close];
            if raw.contains('<') {
                return Err(Unreadable);
            }
            let (prefix, local) = split_name(&name);
            // Attribute values read their white space as spaces.
            let value = decode(&raw.replace(['\t', '\r', '\n'], " "))?;
            attributes.push(XmlAttribute {
                qname: name,
                prefix,
                local,
                ns: None,
                value,
                start: next,
                end: close + 1,
                value_start: j + 1,
                value_end: close,
            });
            i = close + 1;
        }

        let parent = self.open.last().copied();
        if parent.is_none() && !self.elements.is_empty() {
            // A second root.
            return Err(Unreadable);
        }
        let mut scope = parent.map(|p| self.elements[p].scope.clone()).unwrap_or_default();
        for attribute in &attributes {
            if attribute.qname == "xmlns" {
                scope.set("", &attribute.value);
            } else if attribute.prefix == "xmlns" {
                scope.set(&attribute.local, &attribute.value);
            }
        }
        // `xmlns=""` takes the default namespace away: '' reads as none.
        let resolve = |prefix: &str| -> Option<String> {
            if prefix == "xml" {
                return Some(XML_NAMESPACE.to_owned());
            }
            scope.get(prefix).filter(|ns| !ns.is_empty()).map(str::to_owned)
        };
        for attribute in &mut attributes {
            attribute.ns = if attribute.qname == "xmlns" || attribute.prefix == "xmlns" {
                Some(XMLNS_NAMESPACE.to_owned())
            } else if !attribute.prefix.is_empty() {
                resolve(&attribute.prefix)
            } else {
                None
            };
        }
        let start_tag_end = start_tag_close + if self_closing { 2 } else { 1 };
        let (prefix, local) = split_name(&qname);
        let ns = resolve(&prefix);
        let index = self.elements.len();
        self.elements.push(XmlElement {
            qname,
            prefix,
            local,
            ns,
            attributes,
            scope,
            children: Vec::new(),
            text: String::new(),
            start: lt,
            start_tag_close,
            start_tag_end,
            self_closing,
            end_tag_start: start_tag_end,
            end: start_tag_end,
        });
        if let Some(parent) = parent {
            self.elements[parent].children.push(index);
        }
        if !self_closing {
            self.open.push(index);
        }
        self.at = start_tag_end;
        Ok(())
    }

    fn read_end_tag(&mut self, lt: usize) -> Result<(), Unreadable> {
        let qname = self.read_name(lt + 2)?;
        let close = self.skip_space(lt + 2 + qname.len());
        let element = self.open.pop().ok_or(Unreadable)?;
        if self.bytes.get(close) != Some(&b'>') || self.elements[element].qname != qname {
            return Err(Unreadable);
        }
        self.elements[element].end_tag_start = lt;
        self.elements[element].end = close + 1;
        self.at = close + 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elements_keep_where_they_are_and_resolve_their_namespaces() {
        let text = "<?xpacket begin=''?><a xmlns='urn:a' xmlns:b=\"urn:b\"><b:c x='1'>t&amp;u</b:c><d/></a> ";
        let doc = parse_xml(text).expect("readable");
        let root = doc.element(0);
        assert_eq!(root.ns.as_deref(), Some("urn:a"));
        let c = doc.element(root.children[0]);
        assert_eq!(
            (c.ns.as_deref(), c.local.as_str(), c.text.as_str()),
            (Some("urn:b"), "c", "t&u")
        );
        assert_eq!(&text[c.start..c.end], "<b:c x='1'>t&amp;u</b:c>");
        let x = &c.attributes[0];
        assert_eq!((x.ns.as_deref(), x.value.as_str()), (None, "1"));
        assert_eq!(&text[x.value_start..x.value_end], "1");
        let d = doc.element(root.children[1]);
        assert!(d.self_closing);
        assert_eq!(&text[d.start..d.end], "<d/>");
    }

    #[test]
    fn text_that_is_not_well_formed_is_unreadable() {
        for text in [
            "<a>",
            "<a></b>",
            "<a>&nbsp;</a>",
            "<a b='<'/>",
            "<a/><b/>",
            "<a b='1'c='2'/>",
            "",
            "< a/>",
        ] {
            assert!(parse_xml(text).is_none(), "{text}");
        }
    }

    #[test]
    fn character_references_decode_and_escapes_round_trip() {
        let doc = parse_xml("<a>&#x41;&#66;\r\n</a>").unwrap();
        assert_eq!(doc.element(0).text, "AB\n");
        assert!(parse_xml("<a>&#x110000;</a>").is_none());
        assert_eq!(escape_xml_text("a<b>&\r\u{1}"), "a&lt;b&gt;&amp;&#xD;");
        assert_eq!(escape_xml_attribute("\"'\t\n"), "&quot;&apos;&#x9;&#xA;");
    }
}
