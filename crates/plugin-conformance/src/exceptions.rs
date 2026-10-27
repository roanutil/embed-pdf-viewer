//! Named outcome exceptions: a kept Rust divergence that a vector observes.
//!
//! `outcome-exceptions.json` lists each one by vector file, case and the
//! exact callback it moves: Rust logs `callback` in trace step `actualStep`,
//! where the TypeScript logs it in `expectedStep`. Before comparing, the
//! callback is moved back to where the TypeScript has it; everything else in
//! the trace still compares exactly. Every entry carries a `reason` citing
//! the kept divergence and the source.
//!
//! An entry fails loudly when it goes stale: when the trace already matches
//! without it, when the callback is not logged exactly once at `actualStep`,
//! when the TypeScript does not log it at `expectedStep`, or when its case
//! never ran.

use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MovedCallback {
    pub file: String,
    pub case: String,
    pub callback: Value,
    pub actual_step: usize,
    pub expected_step: usize,
    pub reason: String,
}

/// Every entry of `outcome-exceptions.json`. Panics on an invalid list (see
/// [`validate`]).
pub fn all() -> &'static [MovedCallback] {
    static LIST: std::sync::OnceLock<Vec<MovedCallback>> = std::sync::OnceLock::new();
    LIST.get_or_init(|| {
        let list: Vec<MovedCallback> =
            serde_json::from_str(include_str!("../outcome-exceptions.json"))
                .expect("outcome-exceptions.json parses");
        if let Err(why) = validate(&list) {
            panic!("outcome-exceptions.json: {why}");
        }
        list
    })
}

/// An entry needs a reason and two different steps, and lists a callback of
/// its case only once.
pub fn validate(list: &[MovedCallback]) -> Result<(), String> {
    for (index, entry) in list.iter().enumerate() {
        let name = format!("{} / {} / {}", entry.file, entry.case, entry.callback);
        if entry.reason.trim().is_empty() {
            return Err(format!("{name} has no reason"));
        }
        if entry.actual_step == entry.expected_step {
            return Err(format!("{name} moves the callback within step {}", entry.actual_step));
        }
        if list[..index].iter().any(|earlier| {
            earlier.file == entry.file
                && earlier.case == entry.case
                && earlier.callback == entry.callback
        }) {
            return Err(format!("{name} is listed twice"));
        }
    }
    Ok(())
}

/// The entries for one case of `file`.
pub fn for_case(file: &str, case: &str) -> Vec<&'static MovedCallback> {
    all().iter().filter(|e| e.file == file && e.case == case).collect()
}

fn callbacks(trace: &[Value], step: usize) -> Option<&Vec<Value>> {
    trace.get(step)?.get("callbacks")?.as_array()
}

/// Move the listed callback back to where the TypeScript logs it, or say
/// why the entry is stale.
pub fn apply(
    entry: &MovedCallback,
    expected: &[Value],
    actual: &mut [Value],
) -> Result<(), String> {
    let stale = |why: &str| Err(format!("outcome exception is stale: {why}"));
    if crate::compare_trace(expected, actual).is_none() {
        return stale("the trace no longer differs");
    }
    let found: Vec<usize> = callbacks(actual, entry.actual_step)
        .map(|list| {
            list.iter().enumerate().filter(|(_, c)| **c == entry.callback).map(|(i, _)| i).collect()
        })
        .unwrap_or_default();
    let [from] = found[..] else {
        return stale(&format!(
            "{} is logged {} times at step {}",
            entry.callback,
            found.len(),
            entry.actual_step
        ));
    };
    let Some(to) = callbacks(expected, entry.expected_step)
        .and_then(|list| list.iter().position(|c| *c == entry.callback))
    else {
        return stale(&format!(
            "the TypeScript does not log {} at step {}",
            entry.callback, entry.expected_step
        ));
    };
    if callbacks(actual, entry.expected_step).is_none() {
        return stale(&format!("the Rust trace has no step {}", entry.expected_step));
    }
    let moved = actual[entry.actual_step]["callbacks"]
        .as_array_mut()
        .expect("callbacks checked above")
        .remove(from);
    let target =
        actual[entry.expected_step]["callbacks"].as_array_mut().expect("callbacks checked above");
    target.insert(to.min(target.len()), moved);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn step(callbacks: Value) -> Value {
        json!({ "settled": [], "callbacks": callbacks, "status": "started" })
    }

    fn entry() -> MovedCallback {
        MovedCallback {
            file: "f.json".into(),
            case: "c".into(),
            callback: json!({ "fn": "sandbox.dispose", "args": [1] }),
            actual_step: 0,
            expected_step: 1,
            reason: "test".into(),
        }
    }

    #[test]
    fn a_listed_moved_callback_compares_equal() {
        let dispose = json!({ "fn": "sandbox.dispose", "args": [1] });
        let other = json!({ "fn": "sandbox.run", "args": [1] });
        let expected = [step(json!([other])), step(json!([dispose]))];
        let mut actual = vec![step(json!([other, dispose])), step(json!([]))];
        apply(&entry(), &expected, &mut actual).unwrap();
        assert_eq!(crate::compare_trace(&expected, &actual), None);
    }

    #[test]
    fn a_listed_exception_that_no_longer_differs_fails() {
        let dispose = json!({ "fn": "sandbox.dispose", "args": [1] });
        let expected = [step(json!([])), step(json!([dispose]))];
        let mut actual = expected.to_vec();
        assert_eq!(
            apply(&entry(), &expected, &mut actual).unwrap_err(),
            "outcome exception is stale: the trace no longer differs"
        );
    }

    #[test]
    fn an_exception_cannot_hide_any_other_difference() {
        let dispose = json!({ "fn": "sandbox.dispose", "args": [1] });
        let expected = [step(json!([])), step(json!([dispose]))];
        let mut moved_twice = vec![step(json!([dispose, dispose])), step(json!([]))];
        assert!(apply(&entry(), &expected, &mut moved_twice).unwrap_err().contains("2 times"));
        let mut also_wrong = vec![step(json!([dispose])), step(json!([]))];
        also_wrong[1]["status"] = json!("destroyed");
        apply(&entry(), &expected, &mut also_wrong).unwrap();
        assert!(crate::compare_trace(&expected, &also_wrong).is_some());
    }

    #[test]
    fn every_checked_in_entry_has_a_reason() {
        assert!(all().iter().all(|e| !e.reason.trim().is_empty()));
    }

    #[test]
    fn an_entry_within_one_step_is_refused() {
        let same = MovedCallback { expected_step: 0, ..entry() };
        assert!(validate(&[same]).unwrap_err().contains("within step 0"));
    }

    #[test]
    fn a_duplicate_entry_is_refused() {
        assert!(validate(&[entry(), entry()]).unwrap_err().contains("listed twice"));
        let other = MovedCallback { case: "d".into(), ..entry() };
        assert_eq!(validate(&[entry(), other]), Ok(()));
    }

    #[test]
    fn a_missing_expected_step_changes_nothing() {
        let dispose = json!({ "fn": "sandbox.dispose", "args": [1] });
        let expected = [step(json!([])), step(json!([dispose]))];
        let mut actual = vec![step(json!([dispose]))];
        let before = actual.clone();
        assert_eq!(
            apply(&entry(), &expected, &mut actual).unwrap_err(),
            "outcome exception is stale: the Rust trace has no step 1"
        );
        assert_eq!(actual, before);
    }
}
