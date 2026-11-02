//! Editing an XMP packet in place (`metadata/internal/xmp/editXmp.ts`).

use super::xml::{
    Scope, XML_NAMESPACE, XMLNS_NAMESPACE, XmlAttribute, XmlDocument, XmlElement,
    escape_xml_attribute, escape_xml_text, js_space, parse_xml, xml_name,
};
use crate::error::{EngineError, EngineErrorCode};

/// RDF's namespace: an XMP packet's properties sit in its
/// `rdf:Description`s.
pub const RDF_NAMESPACE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

/// A property of an XMP packet, and the prefix it is written with when
/// added.
#[derive(Debug, Clone, PartialEq)]
pub struct XmpProperty {
    pub ns: &'static str,
    pub name: String,
    pub prefix: &'static str,
}

/// A property's value, in the forms document metadata uses.
#[derive(Debug, Clone, PartialEq)]
pub enum XmpValue {
    Text(String),
    /// A language alternative (`rdf:Alt`) holding only its default,
    /// `x-default`.
    Alt(String),
    /// An ordered list (`rdf:Seq`).
    Seq(Vec<String>),
    /// An unordered list (`rdf:Bag`).
    Bag(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct XmpEdit {
    pub property: XmpProperty,
    /// The new value; `None` removes the property.
    pub value: Option<XmpValue>,
    /// Whether a value is added when the packet doesn't have the property;
    /// if not, only one that's there changes.
    pub add: bool,
}

/// `editXmp`: the packet with `edits` made and every other character as it
/// was, or `None` when the packet can't be read (it isn't well-formed XML,
/// or has no `rdf:RDF`).
///
/// A property is a child of one of the top-level `rdf:Description`s, or one
/// of their attributes (the short form of a text value). Removing a
/// property removes it everywhere it appears. Setting one writes it where
/// it first appears, as an attribute still when it is one and the value is
/// text, and removes it everywhere else; when it already reads the new
/// value nothing changes. A property the packet doesn't have goes into the
/// description that declares its namespace, else the first.
pub fn edit_xmp(packet: &str, edits: &[XmpEdit]) -> Result<Option<String>, EngineError> {
    let Some(doc) = parse_xml(packet) else {
        return Ok(None);
    };
    // Document order is the arena's order.
    let Some(rdf) = (0..doc.elements.len()).find(|&i| is_rdf(doc.element(i), "RDF")) else {
        return Ok(None);
    };
    let descriptions: Vec<usize> = doc
        .element(rdf)
        .children
        .iter()
        .copied()
        .filter(|&child| is_rdf(doc.element(child), "Description"))
        .collect();
    let mut writer =
        PacketWriter { packet, doc: &doc, rdf, splices: Vec::new(), additions: Vec::new() };

    for XmpEdit { property, value, add } in edits {
        let elements: Vec<usize> = descriptions
            .iter()
            .flat_map(|&d| doc.element(d).children.iter().copied())
            .filter(|&child| element_names(doc.element(child), property))
            .collect();
        let attributes: Vec<(usize, &XmlAttribute)> = descriptions
            .iter()
            .flat_map(|&d| {
                doc.element(d)
                    .attributes
                    .iter()
                    .filter(|attribute| attribute_names(attribute, property))
                    .map(move |attribute| (d, attribute))
            })
            .collect();

        let Some(value) = value else {
            for &element in &elements {
                writer.remove_element(element);
            }
            for (_, attribute) in &attributes {
                writer.remove_attribute(attribute);
            }
            continue;
        };

        if let Some((&element, other_elements)) = elements.split_first() {
            if !other_elements.is_empty()
                || !attributes.is_empty()
                || !reads(&doc, doc.element(element), value)
            {
                writer.replace_element(element, value);
                for &other in other_elements {
                    writer.remove_element(other);
                }
                for (_, attribute) in &attributes {
                    writer.remove_attribute(attribute);
                }
            }
        } else if let Some(((description, first), other_attributes)) = attributes.split_first() {
            if let XmpValue::Text(text) = value {
                if !other_attributes.is_empty() || first.value != *text {
                    writer.replace_attribute_value(first, text);
                    for (_, attribute) in other_attributes {
                        writer.remove_attribute(attribute);
                    }
                }
            } else {
                for (_, attribute) in &attributes {
                    writer.remove_attribute(attribute);
                }
                writer.add_property(Some(*description), property, value);
            }
        } else if *add {
            let target = descriptions
                .iter()
                .copied()
                .find(|&d| declares(doc.element(d), property.ns))
                .or_else(|| descriptions.first().copied());
            writer.add_property(target, property, value);
        }
    }
    writer.result().map(Some)
}

fn is_rdf(element: &XmlElement, local: &str) -> bool {
    element.ns.as_deref() == Some(RDF_NAMESPACE) && element.local == local
}

fn element_names(element: &XmlElement, property: &XmpProperty) -> bool {
    element.ns.as_deref() == Some(property.ns) && element.local == property.name
}

fn attribute_names(attribute: &XmlAttribute, property: &XmpProperty) -> bool {
    attribute.ns.as_deref() == Some(property.ns) && attribute.local == property.name
}

fn declares(description: &XmlElement, ns: &str) -> bool {
    description
        .attributes
        .iter()
        .any(|attribute| attribute.ns.as_deref() == Some(XMLNS_NAMESPACE) && attribute.value == ns)
}

/// Whether `element` already holds `value`, in the form it would be
/// written.
fn reads(doc: &XmlDocument, element: &XmlElement, value: &XmpValue) -> bool {
    if element.attributes.iter().any(|a| a.ns.as_deref() == Some(RDF_NAMESPACE)) {
        return false;
    }
    let (local, items_wanted): (&str, &[String]) = match value {
        XmpValue::Text(text) => return element.children.is_empty() && element.text == *text,
        XmpValue::Alt(_) => ("Alt", &[]),
        XmpValue::Seq(items) => ("Seq", items),
        XmpValue::Bag(items) => ("Bag", items),
    };
    let [container] = element.children.as_slice() else {
        return false;
    };
    let container = doc.element(*container);
    if !is_rdf(container, local) {
        return false;
    }
    let items: Vec<&XmlElement> = container.children.iter().map(|&i| doc.element(i)).collect();
    if !items.iter().all(|item| is_rdf(item, "li") && item.children.is_empty()) {
        return false;
    }
    if let XmpValue::Alt(text) = value {
        let [item] = items.as_slice() else {
            return false;
        };
        let lang = item
            .attributes
            .iter()
            .find(|a| a.ns.as_deref() == Some(XML_NAMESPACE) && a.local == "lang");
        return lang.is_some_and(|lang| lang.value == "x-default") && item.text == *text;
    }
    items.len() == items_wanted.len()
        && items.iter().zip(items_wanted).all(|(item, wanted)| item.text == *wanted)
}

struct Splice {
    start: usize,
    end: usize,
    text: String,
}

/// What goes into one description: namespace declarations for its start
/// tag, and children.
#[derive(Default)]
struct Additions {
    declarations: Vec<(String, String)>,
    children: Vec<String>,
}

/// Collects the changes to a packet as splices of its text, and writes them
/// out at the end. Additions to one description are gathered first, so its
/// start tag and its end get one change each.
struct PacketWriter<'a> {
    packet: &'a str,
    doc: &'a XmlDocument,
    rdf: usize,
    splices: Vec<Splice>,
    /// By description, in the order first added to; `None` is a description
    /// to be made, for a packet without one.
    additions: Vec<(Option<usize>, Additions)>,
}

impl PacketWriter<'_> {
    /// Removes `element` with the white space before it.
    fn remove_element(&mut self, element: usize) {
        let element = self.doc.element(element);
        self.splices.push(Splice {
            start: self.space_before(element.start),
            end: element.end,
            text: String::new(),
        });
    }

    fn remove_attribute(&mut self, attribute: &XmlAttribute) {
        self.splices.push(Splice {
            start: self.space_before(attribute.start),
            end: attribute.end,
            text: String::new(),
        });
    }

    fn replace_attribute_value(&mut self, attribute: &XmlAttribute, text: &str) {
        self.splices.push(Splice {
            start: attribute.value_start,
            end: attribute.value_end,
            text: escape_xml_attribute(text),
        });
    }

    /// Writes `element` again holding `value`, keeping its name and its own
    /// namespace declarations.
    fn replace_element(&mut self, element: usize, value: &XmpValue) {
        let element = self.doc.element(element);
        let declarations: String = element
            .attributes
            .iter()
            .filter(|a| a.ns.as_deref() == Some(XMLNS_NAMESPACE))
            .map(|a| format!(" {}", &self.packet[a.start..a.end]))
            .collect();
        let rdf_prefix =
            prefix_in(&element.scope, RDF_NAMESPACE, true).unwrap_or_else(|| "rdf".to_owned());
        self.splices.push(Splice {
            start: element.start,
            end: element.end,
            text: property_xml(&element.qname, &declarations, &rdf_prefix, value),
        });
    }

    fn add_property(
        &mut self,
        description: Option<usize>,
        property: &XmpProperty,
        value: &XmpValue,
    ) {
        let scope = description
            .map_or(&self.doc.element(self.rdf).scope, |d| &self.doc.element(d).scope)
            .clone();
        let at = self.additions_for(description);
        let prefix = match prefix_in(&scope, property.ns, false) {
            Some(prefix) => prefix,
            None => declare(&mut self.additions[at].1, &scope, property),
        };
        let rdf_prefix = prefix_in(&scope, RDF_NAMESPACE, true).unwrap_or_else(|| "rdf".to_owned());
        self.additions[at].1.children.push(property_xml(
            &xml_name(&prefix, &property.name),
            "",
            &rdf_prefix,
            value,
        ));
    }

    fn result(mut self) -> Result<String, EngineError> {
        for (description, additions) in std::mem::take(&mut self.additions) {
            self.write_additions(description, &additions);
        }
        // Insertions (empty ranges) go before a change starting at the same
        // place; a stable sort keeps the order they were made in.
        let mut ordered: Vec<&Splice> = self.splices.iter().collect();
        ordered.sort_by_key(|splice| (splice.start, splice.end - splice.start));
        let mut out = String::with_capacity(self.packet.len());
        let mut at = 0;
        for splice in ordered {
            if splice.start < at {
                return Err(EngineError::with_message(
                    EngineErrorCode::Unknown,
                    "XMP edits overlap",
                ));
            }
            out.push_str(&self.packet[at..splice.start]);
            out.push_str(&splice.text);
            at = splice.end;
        }
        out.push_str(&self.packet[at..]);
        Ok(out)
    }

    fn additions_for(&mut self, description: Option<usize>) -> usize {
        match self.additions.iter().position(|(d, _)| *d == description) {
            Some(at) => at,
            None => {
                self.additions.push((description, Additions::default()));
                self.additions.len() - 1
            }
        }
    }

    fn write_additions(&mut self, description: Option<usize>, additions: &Additions) {
        let declarations: String = additions
            .declarations
            .iter()
            .map(|(prefix, ns)| format!(" xmlns:{prefix}=\"{}\"", escape_xml_attribute(ns)))
            .collect();
        let Some(description) = description else {
            // A packet without a description gets one, about the same
            // resource as its siblings would be.
            let rdf_prefix = self.doc.element(self.rdf).prefix.clone();
            let name = xml_name(&rdf_prefix, "Description");
            let made = format!(
                "<{name} {}=\"\"{declarations}>{}</{name}>",
                xml_name(&rdf_prefix, "about"),
                additions.children.concat()
            );
            self.insert_into(self.rdf, made);
            return;
        };
        if !declarations.is_empty() {
            let close = self.doc.element(description).start_tag_close;
            self.splices.push(Splice { start: close, end: close, text: declarations });
        }
        let indent = self.child_indent(description);
        let text: String =
            additions.children.iter().map(|child| format!("{indent}{child}")).collect();
        self.insert_into(description, text);
    }

    /// Puts `text` after `parent`'s last child, turning `<a/>` into
    /// `<a>…</a>`.
    fn insert_into(&mut self, parent: usize, text: String) {
        let parent = self.doc.element(parent);
        if parent.self_closing {
            self.splices.push(Splice {
                start: parent.start_tag_close,
                end: parent.start_tag_end,
                text: format!(">{text}</{}>", parent.qname),
            });
            return;
        }
        let at =
            parent.children.last().map_or(parent.start_tag_end, |&last| self.doc.element(last).end);
        self.splices.push(Splice { start: at, end: at, text });
    }

    /// The white space its children are written after, so an added one
    /// lines up.
    fn child_indent(&self, description: usize) -> String {
        let description = self.doc.element(description);
        let first = description.children.first().map(|&first| self.doc.element(first));
        let before = match first {
            Some(first) => &self.packet[description.start_tag_end..first.start],
            None => &self.packet[description.start_tag_end..description.end_tag_start],
        };
        if !before.chars().all(js_space) {
            return String::new();
        }
        match first {
            Some(_) => before.to_owned(),
            None if before.is_empty() => String::new(),
            None => format!("{before}   "),
        }
    }

    fn space_before(&self, index: usize) -> usize {
        let mut start = index;
        while let Some(c) = self.packet[..start].chars().next_back() {
            if !js_space(c) {
                break;
            }
            start -= c.len_utf8();
        }
        start
    }
}

/// A prefix free in `scope` for the property's namespace, declared on the
/// description.
fn declare(additions: &mut Additions, scope: &Scope, property: &XmpProperty) -> String {
    if let Some((prefix, _)) = additions.declarations.iter().find(|(_, ns)| ns == property.ns) {
        return prefix.clone();
    }
    let mut prefix = property.prefix.to_owned();
    let mut n = 2;
    while scope.has(&prefix) || additions.declarations.iter().any(|(p, _)| *p == prefix) {
        prefix = format!("{}{n}", property.prefix);
        n += 1;
    }
    additions.declarations.push((prefix.clone(), property.ns.to_owned()));
    prefix
}

/// The prefix `ns` has in `scope`; the default namespace (`""`) only when
/// `allow_default`.
fn prefix_in(scope: &Scope, ns: &str, allow_default: bool) -> Option<String> {
    let mut found = None;
    for (prefix, bound) in scope.iter() {
        if bound != ns {
            continue;
        }
        if !prefix.is_empty() {
            return Some(prefix.to_owned());
        }
        if allow_default {
            found = Some(String::new());
        }
    }
    found
}

fn property_xml(name: &str, declarations: &str, rdf_prefix: &str, value: &XmpValue) -> String {
    let rdf = |local: &str| xml_name(rdf_prefix, local);
    let item = |text: &str, attributes: &str| {
        format!("<{li}{attributes}>{}</{li}>", escape_xml_text(text), li = rdf("li"))
    };
    let container = |local: &str, items: String| format!("<{c}>{items}</{c}>", c = rdf(local));
    let inner = match value {
        XmpValue::Text(text) => escape_xml_text(text),
        XmpValue::Alt(text) => container("Alt", item(text, " xml:lang=\"x-default\"")),
        XmpValue::Seq(items) => container("Seq", items.iter().map(|text| item(text, "")).collect()),
        XmpValue::Bag(items) => container("Bag", items.iter().map(|text| item(text, "")).collect()),
    };
    format!("<{name}{declarations}>{inner}</{name}>")
}
