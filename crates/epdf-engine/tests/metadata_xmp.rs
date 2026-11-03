//! The XMP packet kept in step with metadata updates, against upstream's
//! `main/test/metadata-xmp.test.ts`: first its `editXmp` cases, text for
//! text, then the engine cases over the same fixtures through `Host::call`.

use epdf_engine::features::metadata_write::check_custom_keys;
use epdf_engine::features::xmp::edit::edit_xmp;
use epdf_engine::features::xmp::{
    read_xmp_packet, xmp_edits_for_custom_metadata, xmp_edits_for_metadata,
};
use epdf_engine::host::Host;
use serde_json::{Value, json};

const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const DC: &str = "http://purl.org/dc/elements/1.1/";
const XMP: &str = "http://ns.adobe.com/xap/1.0/";
const PDF: &str = "http://ns.adobe.com/pdf/1.3/";
const PDFX: &str = "http://ns.adobe.com/pdfx/1.3/";

/// An XMP packet around `descriptions`, laid out the way Adobe's toolkit
/// writes one.
fn packet(descriptions: &str) -> String {
    format!(
        "<?xpacket begin=\"\u{FEFF}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Adobe XMP Core 9.1\">\n   \
         <rdf:RDF xmlns:rdf=\"{RDF}\">{descriptions}\n   </rdf:RDF>\n</x:xmpmeta>\n{}\n\
         <?xpacket end=\"w\"?>",
        " ".repeat(40)
    )
}

fn title(text: &str) -> String {
    format!(
        "<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{text}</rdf:li></rdf:Alt></dc:title>"
    )
}

fn edit(before: &str, patch: Value) -> Option<String> {
    let patch = serde_json::from_value(patch).expect("a metadata patch");
    edit_xmp(before, &xmp_edits_for_metadata(&patch).unwrap()).unwrap()
}

#[test]
fn adds_rewrites_and_removes_properties_leaving_the_rest_of_the_packet_as_written() {
    let before = packet(&format!(
        r#"
      <rdf:Description rdf:about=""
            xmlns:xmp="{XMP}"
            xmlns:dc="{DC}"
            xmlns:pdf="{PDF}">
         <xmp:CreatorTool>Adobe Acrobat 26.2</xmp:CreatorTool>
         <dc:format>application/pdf</dc:format>
         <pdf:Producer>Adobe Acrobat 26.2</pdf:Producer>
      </rdf:Description>"#
    ));
    let after = packet(&format!(
        r#"
      <rdf:Description rdf:about=""
            xmlns:xmp="{XMP}"
            xmlns:dc="{DC}"
            xmlns:pdf="{PDF}">
         <dc:format>application/pdf</dc:format>
         {}
      </rdf:Description>"#,
        title("NEW")
    ));
    assert_eq!(
        edit(&before, json!({ "creator": null, "producer": null, "title": "NEW" })),
        Some(after)
    );
}

#[test]
fn writes_each_property_in_the_description_of_its_schema() {
    let before = packet(&format!(
        r#"
      <rdf:Description rdf:about=""
            xmlns:pdf="{PDF}">
         <pdf:Producer>Acrobat Distiller 10.0.0 (Windows)</pdf:Producer>
      </rdf:Description>
      <rdf:Description rdf:about=""
            xmlns:dc="{DC}">
         <dc:format>application/pdf</dc:format>
         <dc:title>
            <rdf:Alt>
               <rdf:li xml:lang="x-default">1.txt - Notepad</rdf:li>
               <rdf:li xml:lang="de">1.txt - Editor</rdf:li>
            </rdf:Alt>
         </dc:title>
         <dc:creator>
            <rdf:Seq>
               <rdf:li>User</rdf:li>
            </rdf:Seq>
         </dc:creator>
      </rdf:Description>"#
    ));
    let after = packet(&format!(
        r#"
      <rdf:Description rdf:about=""
            xmlns:pdf="{PDF}">
         <pdf:Keywords>red, green</pdf:Keywords>
      </rdf:Description>
      <rdf:Description rdf:about=""
            xmlns:dc="{DC}">
         <dc:format>application/pdf</dc:format>
         {}
         <dc:creator><rdf:Seq><rdf:li>Dana Smith</rdf:li></rdf:Seq></dc:creator>
      </rdf:Description>"#,
        title("Q2")
    ));
    assert_eq!(
        edit(
            &before,
            json!({ "title": "Q2", "author": "Dana Smith", "producer": null,
                "keywords": "red, green" })
        ),
        Some(after)
    );
}

#[test]
fn keeps_the_short_form_matches_by_namespace_and_opens_a_self_closed_description() {
    let before = packet(&format!(
        r#"
      <rdf:Description rdf:about="" xmlns:pdf="{PDF}" xmlns:xap="{XMP}" pdf:Producer="Old" xap:CreatorTool="Tool"/>"#
    ));
    let after = packet(&format!(
        r#"
      <rdf:Description rdf:about="" xmlns:pdf="{PDF}" xmlns:xap="{XMP}" pdf:Producer="say &quot;hi&quot;" xmlns:dc="{DC}">{}</rdf:Description>"#,
        title("T")
    ));
    assert_eq!(
        edit(&before, json!({ "producer": "say \"hi\"", "creator": null, "title": "T" })),
        Some(after)
    );
}

#[test]
fn leaves_a_packet_that_already_reads_the_values_as_it_is() {
    let before = packet(&format!(
        r#"
      <rdf:Description rdf:about="" xmlns:dc="{DC}" xmlns:pdf="{PDF}">
         <dc:title>
            <rdf:Alt>
               <rdf:li xml:lang="x-default">A &amp; B</rdf:li>
            </rdf:Alt>
         </dc:title>
         <pdf:Producer><![CDATA[<made>]]></pdf:Producer>
      </rdf:Description>"#
    ));
    assert_eq!(
        edit(&before, json!({ "title": "A & B", "producer": "<made>", "creator": null })),
        Some(before)
    );
}

#[test]
fn a_property_in_several_descriptions_is_written_once_and_removed_elsewhere() {
    let before = packet(&format!(
        r#"
      <rdf:Description rdf:about="" xmlns:dc="{DC}">
         {}
      </rdf:Description>
      <rdf:Description rdf:about="" xmlns:dc="{DC}">
         {}
      </rdf:Description>"#,
        title("One"),
        title("Two")
    ));
    let after = packet(&format!(
        r#"
      <rdf:Description rdf:about="" xmlns:dc="{DC}">
         {}
      </rdf:Description>
      <rdf:Description rdf:about="" xmlns:dc="{DC}">
      </rdf:Description>"#,
        title("Three")
    ));
    assert_eq!(edit(&before, json!({ "title": "Three" })), Some(after));
}

#[test]
fn escapes_text_and_drops_characters_xml_cannot_hold() {
    let before = packet(&format!(
        r#"
      <rdf:Description rdf:about="" xmlns:dc="{DC}">
         <dc:format>application/pdf</dc:format>
      </rdf:Description>"#
    ));
    let after = edit(&before, json!({ "title": "A & B <c>\u{1}\r" })).unwrap();
    assert!(after.contains(&title("A &amp; B &lt;c&gt;&#xD;")), "{after}");
}

#[test]
fn makes_a_description_when_the_packet_has_none() {
    let before = format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="{RDF}"></rdf:RDF></x:xmpmeta>"#
    );
    let after = format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="{RDF}"><rdf:Description rdf:about="" xmlns:dc="{DC}">{}</rdf:Description></rdf:RDF></x:xmpmeta>"#,
        title("T")
    );
    assert_eq!(edit(&before, json!({ "title": "T", "creator": null })), Some(after));
}

#[test]
fn declares_a_free_prefix_when_the_usual_one_is_taken() {
    let before = packet(
        r#"
      <rdf:Description rdf:about="" xmlns:dc="http://example.com/not-dublin-core/">
         <dc:format>x</dc:format>
      </rdf:Description>"#,
    );
    let after = packet(&format!(
        r#"
      <rdf:Description rdf:about="" xmlns:dc="http://example.com/not-dublin-core/" xmlns:dc2="{DC}">
         <dc:format>x</dc:format>
         <dc2:title><rdf:Alt><rdf:li xml:lang="x-default">T</rdf:li></rdf:Alt></dc2:title>
      </rdf:Description>"#
    ));
    assert_eq!(edit(&before, json!({ "title": "T" })), Some(after));
}

#[test]
fn keywords_dates_and_trapping() {
    let before = packet(&format!(
        r#"
      <rdf:Description rdf:about="" xmlns:dc="{DC}" xmlns:xmp="{XMP}">
         <dc:subject>
            <rdf:Bag>
               <rdf:li>old</rdf:li>
            </rdf:Bag>
         </dc:subject>
         <xmp:CreateDate>2017-07-12T21:44:10-07:00</xmp:CreateDate>
      </rdf:Description>"#
    ));
    let after = edit(
        &before,
        json!({ "keywords": "red, green; blue", "createdAt": "2026-01-04T09:00:00.250+01:00",
            "modifiedAt": "2026-01-05T10:00:00.500Z", "trapped": "true" }),
    )
    .unwrap();
    for part in [
        "<dc:subject><rdf:Bag><rdf:li>red</rdf:li><rdf:li>green</rdf:li><rdf:li>blue</rdf:li></rdf:Bag></dc:subject>",
        // Dates as /Info gets them: whole seconds, the same offset.
        "<xmp:CreateDate>2026-01-04T09:00:00+01:00</xmp:CreateDate>",
        "<xmp:ModifyDate>2026-01-05T10:00:00Z</xmp:ModifyDate>",
        "<pdf:Keywords>red, green; blue</pdf:Keywords>",
        "<pdf:Trapped>True</pdf:Trapped>",
    ] {
        assert!(after.contains(part), "{part} in {after}");
    }

    // The keyword list follows keywords only where the packet has one.
    let without = packet(&format!(
        r#"
      <rdf:Description rdf:about="" xmlns:pdf="{PDF}"/>"#
    ));
    assert!(!edit(&without, json!({ "keywords": "a, b" })).unwrap().contains("dc:subject"));
    assert!(!edit(&before, json!({ "keywords": null })).unwrap().contains("dc:subject"));
}

#[test]
fn custom_keys_follow_their_pdfx_property_which_is_never_added() {
    let before = packet(&format!(
        r#"
      <rdf:Description rdf:about="" xmlns:pdfx="{PDFX}" pdfx:SourceModified="D:2008">
         <pdfx:Company>Fernquill Ltd</pdfx:Company>
      </rdf:Description>"#
    ));
    let patch = serde_json::from_value(json!({ "Company": null, "SourceModified": "D:2026",
        "reviewedBy": "lee", "has space": null }))
    .unwrap();
    let edits = xmp_edits_for_custom_metadata(&patch);
    let mut names: Vec<&str> = edits.iter().map(|edit| edit.property.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["Company", "SourceModified", "reviewedBy"]);
    let after = packet(&format!(
        r#"
      <rdf:Description rdf:about="" xmlns:pdfx="{PDFX}" pdfx:SourceModified="D:2026">
      </rdf:Description>"#
    ));
    assert_eq!(edit_xmp(&before, &edits).unwrap(), Some(after));
}

#[test]
fn a_packet_that_is_not_well_formed_or_has_no_rdf_rdf_is_not_read() {
    let patch = json!({ "title": "T" });
    assert_eq!(edit("<x:xmpmeta><rdf:RDF", patch.clone()), None);
    assert_eq!(edit(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"/>"#, patch.clone()), None);
    assert_eq!(
        edit(&packet(r#"<rdf:Description rdf:about="">&nbsp;</rdf:Description>"#), patch.clone()),
        None
    );
    assert_eq!(edit(&packet("<rdf:Description></rdf:Descriptio>"), patch), None);
}

/* ── through the engine ─────────────────────────────────────────────────── */

const FIXTURES: &str = "packages/engine/main/test/fixtures";
const TOOL: &str = "Adobe Acrobat 26.2.21931";

fn open(bytes: Vec<u8>) -> Host {
    let mut host = Host::new();
    let response = host.call_filled(
        &json!({ "kind": "open.fatMem", "jobId": 1, "docId": "d" }).to_string(),
        vec![bytes],
    );
    assert!(!response.json.contains("\"error\""), "{}", response.json);
    host
}

fn send(host: &mut Host, mut request: Value) -> (Value, Vec<Vec<u8>>) {
    request["jobId"] = json!(7);
    request["docId"] = json!("d");
    let response = host.call_filled(&request.to_string(), Vec::new());
    let envelope: Value = serde_json::from_str(&response.json).expect("JSON");
    assert!(envelope.get("error").is_none(), "{request}: {envelope}");
    (envelope["result"].clone(), response.payloads)
}

fn update(host: &mut Host, op_id: &str, patch: Value) -> Value {
    send(host, json!({ "kind": "metadata.update", "opId": op_id, "patch": patch })).0
}

fn update_custom(host: &mut Host, op_id: &str, patch: Value) -> Value {
    send(host, json!({ "kind": "metadata.updateCustom", "opId": op_id, "patch": patch })).0
}

fn download(host: &mut Host, mode: &str) -> Vec<u8> {
    send(host, json!({ "kind": "document.saveBuffer", "mode": mode })).1.remove(0)
}

/// The XMP packet a saved file opens with.
fn xmp_of(bytes: &[u8]) -> Option<String> {
    let document = pdfium::Document::from_bytes(pdfium::Library::acquire(), bytes, None)
        .expect("the saved file opens");
    read_xmp_packet(&document).unwrap().map(|packet| packet.text)
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// A PDF of `objects` (numbered from 1), with `trailer` added to the
/// trailer dictionary.
fn pdf(objects: &[Vec<u8>], trailer: &str) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend(object);
        out.extend(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R {trailer} >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn with_packet(packet: &[u8], info: Option<&str>) -> Vec<u8> {
    let mut stream =
        format!("<< /Type /Metadata /Subtype /XML /Length {} >>\nstream\n", packet.len())
            .into_bytes();
    stream.extend(packet);
    stream.extend(b"\nendstream");
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R /Metadata 4 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_vec(),
        stream,
    ];
    if let Some(info) = info {
        objects.push(info.as_bytes().to_vec());
    }
    pdf(&objects, if info.is_some() { "/Info 5 0 R" } else { "" })
}

#[test]
fn removing_the_creator_and_producer_removes_them_from_the_xmp_too() {
    for mode in ["rewrite", "incremental"] {
        let mut host =
            open(test_support::fixture(&format!("{FIXTURES}/stamp-opacity-acrobat.pdf")));
        assert!(
            xmp_of(&download(&mut host, "rewrite"))
                .unwrap()
                .contains(&format!("<xmp:CreatorTool>{TOOL}</xmp:CreatorTool>"))
        );
        update(&mut host, "u", json!({ "creator": null, "producer": null, "title": "NEW" }));
        let saved = download(&mut host, mode);
        let xmp = xmp_of(&saved).unwrap();
        assert!(xmp.contains(&title("NEW")), "{mode}");
        assert!(!xmp.contains("CreatorTool"), "{mode}");
        assert!(!xmp.contains("Producer"), "{mode}");
        // The rest stays as Acrobat wrote it.
        assert!(xmp.contains(
            "<xmpMM:DocumentID>uuid:38186d3c-b880-ca40-a1ee-d886f550f0f5</xmpMM:DocumentID>"
        ));
        assert!(xmp.contains("<?xpacket end=\"w\"?>"));
        // A rewrite holds no trace of the old values; an incremental save
        // keeps the old revision, the packet included.
        assert_eq!(latin1(&saved).contains(TOOL), mode == "incremental", "{mode}");

        let mut reopened = open(saved);
        let read = send(&mut reopened, json!({ "kind": "metadata.read" })).0["metadata"].clone();
        assert_eq!(read["title"], "NEW", "{mode}");
        assert!(read["creator"].is_null() && read["producer"].is_null(), "{mode}: {read}");
    }
}

#[test]
fn a_packet_with_one_description_per_schema() {
    for mode in ["rewrite", "incremental"] {
        let mut host = open(test_support::fixture(&format!("{FIXTURES}/embedded_attachments.pdf")));
        update(&mut host, "u", json!({ "author": "Dana Smith", "title": null, "createdAt": null }));
        let xmp = xmp_of(&download(&mut host, mode)).unwrap();
        assert!(
            xmp.contains("<dc:creator><rdf:Seq><rdf:li>Dana Smith</rdf:li></rdf:Seq></dc:creator>")
        );
        assert!(!xmp.contains("dc:title"), "{mode}");
        assert!(!xmp.contains("xmp:CreateDate"), "{mode}");
        assert!(xmp.contains("<xmp:CreatorTool>PScript5.dll Version 5.2.2</xmp:CreatorTool>"));
        assert!(xmp.contains("<pdf:Producer>Acrobat Distiller 10.0.0 (Windows)</pdf:Producer>"));
    }
}

#[test]
fn an_encrypted_packet_is_read_and_written_through_the_encryption() {
    let mut host = open(test_support::fixture(&format!("{FIXTURES}/uscis-i-140-protected.pdf")));
    assert!(xmp_of(&download(&mut host, "rewrite")).unwrap().contains("x:xmpmeta"));
    update(&mut host, "u", json!({ "title": "NEW" }));
    assert!(xmp_of(&download(&mut host, "rewrite")).unwrap().contains("NEW</rdf:li>"));
}

#[test]
fn a_document_without_xmp_gets_none() {
    let mut host = open(test_support::fixture(&format!("{FIXTURES}/hello_world.pdf")));
    update(&mut host, "u", json!({ "title": "NEW", "author": "Dana" }));
    update_custom(&mut host, "c", json!({ "reviewedBy": "lee" }));
    let saved = download(&mut host, "rewrite");
    assert_eq!(xmp_of(&saved), None);
    assert!(!latin1(&saved).contains("/Metadata"));
}

#[test]
fn a_utf16_packet_goes_back_in_utf16() {
    let text = format!(
        "\u{FEFF}<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"{RDF}\">\
         <rdf:Description rdf:about=\"\" xmlns:pdf=\"{PDF}\"><pdf:Producer>Old</pdf:Producer>\
         </rdf:Description></rdf:RDF></x:xmpmeta>"
    );
    let utf16 =
        |text: &str| -> Vec<u8> { text.encode_utf16().flat_map(u16::to_le_bytes).collect() };
    let mut host = open(with_packet(&utf16(&text), None));
    update(&mut host, "u", json!({ "producer": "Ünïcode" }));
    let saved = download(&mut host, "rewrite");
    assert_eq!(xmp_of(&saved), Some(text.replace(">Old<", ">Ünïcode<")));
    assert!(latin1(&saved).contains(&latin1(&utf16("<pdf:Producer>Ünïcode"))));
}

#[test]
fn custom_keys_reach_the_pdfx_properties_adobe_apps_copy_them_to() {
    let xmp = format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"{RDF}\">\
         <rdf:Description rdf:about=\"\" xmlns:pdfx=\"{PDFX}\">\
         <pdfx:Company>Fernquill Ltd</pdfx:Company><pdfx:Project>Atlas</pdfx:Project>\
         </rdf:Description></rdf:RDF></x:xmpmeta>"
    );
    let mut host =
        open(with_packet(xmp.as_bytes(), Some("<< /Company (Fernquill Ltd) /Project (Atlas) >>")));
    update_custom(
        &mut host,
        "c",
        json!({ "Company": null, "Project": "Beacon", "reviewedBy": "lee" }),
    );
    let saved = download(&mut host, "rewrite");
    assert_eq!(
        xmp_of(&saved),
        Some(
            xmp.replace("<pdfx:Company>Fernquill Ltd</pdfx:Company>", "")
                .replace("Atlas", "Beacon")
        )
    );
    assert!(!latin1(&saved).contains("Fernquill"));
}

#[test]
fn an_undo_puts_the_xmp_back_with_the_metadata() {
    let mut host = open(test_support::fixture(&format!("{FIXTURES}/embedded_attachments.pdf")));
    update(&mut host, "u", json!({ "title": "Changed", "author": null }));
    assert!(xmp_of(&download(&mut host, "rewrite")).unwrap().contains(&title("Changed")));
    send(
        &mut host,
        json!({ "kind": "document.apply", "opId": "undo", "change": { "undoOf": "u" } }),
    );
    let xmp = xmp_of(&download(&mut host, "rewrite")).unwrap();
    assert!(xmp.contains(&title("1.txt - Notepad")), "{xmp}");
    assert!(xmp.contains("<dc:creator><rdf:Seq><rdf:li>User</rdf:li></rdf:Seq></dc:creator>"));
}

#[test]
fn a_refused_custom_key_changes_neither_info_nor_xmp() {
    // The key check runs before anything is written, the packet included.
    let keys = ["Title".to_owned()];
    assert!(check_custom_keys(keys.iter()).is_err());
}
