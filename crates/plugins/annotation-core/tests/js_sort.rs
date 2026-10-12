//! `js::sort_by` against V8: `fixtures/js-sort.json` records what V8's
//! `Array.prototype.sort` does with `(a, b) => k[a] - k[b]` over keys with
//! NaN in them, a comparator that is not a total order. `js-sort.mjs`
//! regenerates it; both sides rebuild each case's keys from one mulberry32
//! stream.

use epdf_annotation_core::js::{sort_by, sort_by_ordering};
use serde_json::Value;

fn mulberry32(mut seed: u32) -> impl FnMut() -> u32 {
    move || {
        seed = seed.wrapping_add(0x6d2b79f5);
        let mut t = (seed ^ (seed >> 15)).wrapping_mul(1 | seed);
        t = (t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t))) ^ t;
        t ^ (t >> 14)
    }
}

fn keys(kind: &str, n: usize, seed: u32) -> Vec<f64> {
    let mut next = mulberry32(seed);
    let mut out = Vec::with_capacity(n);
    match kind {
        "noisy" => {
            for _ in 0..n {
                let r = next();
                out.push(if r.is_multiple_of(5) { f64::NAN } else { (r % 20) as f64 });
            }
        }
        "runs" => {
            let mut v = 0.0;
            while out.len() < n {
                let len = 1 + next() % 200;
                let down = next().is_multiple_of(4);
                let mut i = 0;
                while i < len && out.len() < n {
                    let r = next();
                    let step = (r % 3) as f64;
                    v += if down { -step } else { step };
                    out.push(if r.is_multiple_of(100) { f64::NAN } else { v });
                    i += 1;
                }
            }
        }
        "sorted-nan" => {
            for i in 0..n {
                out.push(if i % 37 == 36 { f64::NAN } else { i as f64 });
            }
        }
        _ => {
            for _ in 0..n {
                out.push((next() % 1000) as f64);
            }
        }
    }
    out
}

fn hash(order: &[usize]) -> u32 {
    order.iter().fold(2166136261u32, |h, &i| (h ^ i as u32).wrapping_mul(16777619))
}

#[test]
fn sort_by_leaves_elements_where_v8_does() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/js-sort.json")).expect("the fixture");
    let cases = fixture["cases"].as_array().expect("cases");
    assert!(cases.len() > 50);
    for case in cases {
        let kind = case["kind"].as_str().unwrap();
        let n = case["n"].as_u64().unwrap() as usize;
        let seed = case["seed"].as_u64().unwrap() as u32;
        let k = keys(kind, n, seed);
        let mut order: Vec<usize> = (0..n).collect();
        sort_by(&mut order, |a, b| k[*a] - k[*b]);
        assert_eq!(u64::from(hash(&order)), case["hash"].as_u64().unwrap(), "{kind} {n}");
    }
}

#[test]
fn a_consistent_comparator_gets_the_stable_order() {
    let k = keys("clean", 3000, 42);
    let mut ours: Vec<usize> = (0..k.len()).collect();
    sort_by_ordering(&mut ours, |a, b| k[*a].total_cmp(&k[*b]));
    let mut std: Vec<usize> = (0..k.len()).collect();
    std.sort_by(|a, b| k[*a].total_cmp(&k[*b]));
    assert_eq!(ours, std);
}
