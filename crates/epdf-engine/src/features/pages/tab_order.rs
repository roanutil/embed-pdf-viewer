//! Annotation keyboard order in top-left page space, following the page's PDF /Tabs policy.
use serde_json::Value;
use std::cmp::Ordering;
/// Compare tag paths lexicographically, with a parent before its children.
pub fn compare_structure_positions(left: &[u32], right: &[u32]) -> Ordering {
    left.cmp(right)
}
/// Put annotations in document page order and each page's keyboard order.
/// Rows on absent pages are omitted. Ties preserve input order.
pub fn tab_order_of(annotations: &[Value], pages: &[Value]) -> Vec<Value> {
    let mut result = Vec::with_capacity(annotations.len());
    for page in pages {
        let page_number = page["ref"]["objectNumber"].as_f64();
        let mut rows: Vec<_> = annotations
            .iter()
            .filter(|row| row["page"]["objectNumber"].as_f64() == page_number)
            .collect();
        match page["tabOrder"].as_str() {
            Some("rows") => rows = in_lines(rows, "y", "x", "height"),
            Some("columns") => rows = in_lines(rows, "x", "y", "width"),
            Some("structure") => {
                let mut tagged: Vec<_> =
                    rows.iter().copied().filter(|r| position(r).is_some()).collect();
                tagged.sort_by(|left, right| {
                    compare_structure_positions(&position(left).unwrap(), &position(right).unwrap())
                });
                tagged.extend(rows.iter().copied().filter(|r| position(r).is_none()));
                rows = tagged;
            }
            Some("widgets") => {
                let mut widgets: Vec<_> =
                    rows.iter().copied().filter(|r| r["subtype"] == "widget").collect();
                widgets.extend(rows.iter().copied().filter(|r| r["subtype"] != "widget"));
                rows = widgets;
            }
            _ => {}
        }
        result.extend(rows.into_iter().cloned());
    }
    result
}
fn position(row: &Value) -> Option<Vec<u32>> {
    row.get("structurePosition")?
        .as_array()?
        .iter()
        .map(|n| n.as_u64().and_then(|n| u32::try_from(n).ok()))
        .collect()
}
fn coordinate(row: &Value, key: &str) -> f64 {
    row["rect"][key].as_f64().unwrap_or(f64::NAN)
}
fn compare(left: f64, right: f64) -> Ordering {
    left.partial_cmp(&right).unwrap_or(Ordering::Equal)
}
fn in_lines<'a>(mut rows: Vec<&'a Value>, across: &str, along: &str, size: &str) -> Vec<&'a Value> {
    rows.sort_by(|left, right| compare(coordinate(left, across), coordinate(right, across)));
    let mut lines: Vec<Vec<&Value>> = Vec::new();
    let mut end = f64::NEG_INFINITY;
    for row in rows {
        let middle = coordinate(row, across) + coordinate(row, size) / 2.0;
        if let Some(line) = lines.last_mut().filter(|_| middle <= end) {
            line.push(row);
        } else {
            end = coordinate(row, across) + coordinate(row, size);
            lines.push(vec![row]);
        }
    }
    lines
        .into_iter()
        .flat_map(|mut line| {
            line.sort_by(|left, right| compare(coordinate(left, along), coordinate(right, along)));
            line
        })
        .collect()
}
