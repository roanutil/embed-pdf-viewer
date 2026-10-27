//! `suggestions.ts`: "Did you mean …?" for ids. The closest known id to one
//! that doesn't exist, by edit distance, when it is close enough to be a
//! typo. Distances and lengths count UTF-16 units, as the TypeScript's
//! string indexing does.

/// The number of single-unit inserts, deletes, substitutions and swaps of two
/// neighbours that turn `left` into `right`.
pub fn edit_distance(left: &str, right: &str) -> usize {
    if left == right {
        return 0;
    }
    let left: Vec<u16> = left.encode_utf16().collect();
    let right: Vec<u16> = right.encode_utf16().collect();
    let mut before_previous: Vec<usize> = Vec::new();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    for i in 1..=left.len() {
        let mut current = vec![i];
        for j in 1..=right.len() {
            let cost = usize::from(left[i - 1] != right[j - 1]);
            let mut distance =
                (previous[j] + 1).min(current[j - 1] + 1).min(previous[j - 1] + cost);
            if i > 1 && j > 1 && left[i - 1] == right[j - 2] && left[i - 2] == right[j - 1] {
                distance = distance.min(before_previous[j - 2] + 1);
            }
            current.push(distance);
        }
        before_previous = previous;
        previous = current;
    }
    previous[right.len()]
}

/// The candidate closest to `id`, or `None` when none is within a typo's
/// reach: at most a third of the id's length, and never more than 3 edits
/// apart. The first of equally close candidates wins.
pub fn closest_match<'a>(
    id: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let reach = (crate::js::utf16_len(id) / 3).clamp(1, 3);
    let mut best = None;
    let mut best_distance = reach + 1;
    for candidate in candidates {
        if candidate == id {
            continue;
        }
        let distance = edit_distance(id, candidate);
        if distance < best_distance {
            best = Some(candidate);
            best_distance = distance;
        }
    }
    best
}

/// `' (did you mean "x"?)'` for the closest candidate, or `""` when there is
/// none.
pub fn did_you_mean<'a>(id: &str, candidates: impl IntoIterator<Item = &'a str>) -> String {
    match closest_match(id, candidates) {
        Some(candidate) => format!(" (did you mean \"{candidate}\"?)"),
        None => String::new(),
    }
}
