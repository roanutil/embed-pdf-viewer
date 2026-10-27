//! `actions.ts`: executable JavaScript out of an extracted action tree.

use epdf_kernel::Error;
use serde_json::Value;

use crate::types::{PdfActionNode, PdfActionTree};

/// JavaScript sources in PDF action order: root, then `/Next` depth-first.
/// An incomplete tree is never partially executed.
pub fn java_script_sources_from_action_tree(tree: &PdfActionTree) -> Result<Vec<String>, Error> {
    if truthy(tree.get("incomplete")) {
        return Err(Error::new("Refusing to execute an incomplete PDF action tree"));
    }
    fn visit(node: &PdfActionNode, sources: &mut Vec<String>) {
        // Only the javascript arm is collected; a rendition's `/JS` waits for
        // a rendition interpreter.
        if node.get("type").and_then(Value::as_str) == Some("javascript") {
            sources
                .push(node.get("script").and_then(Value::as_str).unwrap_or_default().to_string());
        }
        for child in node.get("next").and_then(Value::as_array).into_iter().flatten() {
            visit(child, sources);
        }
    }
    let mut sources = Vec::new();
    if let Some(root) = tree.get("root").filter(|root| truthy(Some(root))) {
        visit(root, &mut sources);
    }
    Ok(sources)
}

/// One event source that keeps the tree's shared field overlay.
pub fn java_script_program_from_action_tree(tree: &PdfActionTree) -> Result<String, Error> {
    Ok(java_script_sources_from_action_tree(tree)?.join("\n;\n"))
}

/// JavaScript truthiness of a JSON member (absent is `undefined`).
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(other) => !epdf_kernel::is_undefined(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `actions.ts`: a poisoned native snapshot is never partially executed,
    /// whichever entry point reads it.
    #[test]
    fn an_incomplete_tree_yields_no_script_at_all() {
        let leaf = json!({ "type": "javascript", "script": "deep()", "next": [] });
        let tree = json!({
            "root": { "type": "javascript", "script": "first()", "next": [leaf] },
            "incomplete": true,
        });
        let message = "Refusing to execute an incomplete PDF action tree";
        assert_eq!(java_script_sources_from_action_tree(&tree).unwrap_err().message, message);
        assert_eq!(java_script_program_from_action_tree(&tree).unwrap_err().message, message);
        let complete = json!({ "root": tree["root"], "incomplete": false });
        assert_eq!(java_script_program_from_action_tree(&complete).unwrap(), "first()\n;\ndeep()");
    }
}
