//! `negotiate.ts`: pure BCP-47 locale negotiation. The embedder feeds it
//! whatever request list its environment offers.
//!
//! Per requested code in preference order: an exact match (case-
//! insensitive), then the request narrowed to its language, then any
//! available dialect of the language. Returns the matched code exactly as it
//! appears in `available`.

fn language_of(code: &str) -> String {
    let lower = code.to_lowercase();
    match lower.split_once('-') {
        Some((language, _)) => language.to_string(),
        None => lower,
    }
}

pub fn negotiate_locale<A: AsRef<str>, R: AsRef<str>>(
    available: &[A],
    requested: &[R],
) -> Option<String> {
    // `Map<canon, original>` in insertion order, first spelling wins.
    let mut by_canon: Vec<(String, &str)> = Vec::new();
    for code in available {
        let code = code.as_ref();
        let canon = code.to_lowercase();
        if !by_canon.iter().any(|(c, _)| *c == canon) {
            by_canon.push((canon, code));
        }
    }
    let find = |canon: &str| by_canon.iter().find(|(c, _)| c == canon).map(|(_, o)| o.to_string());

    for want in requested {
        if let Some(exact) = find(&want.as_ref().to_lowercase()) {
            return Some(exact);
        }
    }
    for want in requested {
        if let Some(narrowed) = find(&language_of(want.as_ref())) {
            return Some(narrowed);
        }
    }
    for want in requested {
        let language = language_of(want.as_ref());
        for (canon, original) in &by_canon {
            if language_of(canon) == language {
                return Some(original.to_string());
            }
        }
    }
    None
}
