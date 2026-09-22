use std::{fs, hint::black_box, time::Instant};

fn measure(mut operation: impl FnMut() -> usize, iterations: usize) -> (f64, usize) {
    let start = Instant::now();
    let mut checksum = 0usize;
    for _ in 0..iterations {
        checksum = checksum.wrapping_add(black_box(operation()));
    }
    (start.elapsed().as_nanos() as f64 / iterations as f64, checksum)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: native_bench <pdf> [iterations]");
    let iterations = args.next().and_then(|v| v.parse().ok()).unwrap_or(20).max(1);
    let engine = epdf_ops::Engine::new().unwrap();
    let document = engine.open(fs::read(path).unwrap(), None).unwrap();
    let page = 4;
    let (page_count_ns, page_count_sum) =
        measure(|| document.page_count().unwrap() as usize, iterations);
    let (page_size_ns, page_size_sum) = measure(
        || document.page_size(page).unwrap().width as usize,
        iterations,
    );
    let (text_ns, text_sum) = measure(
        || document.page_text(page).unwrap().encode_utf16().count(),
        iterations,
    );
    let (search_ns, search_sum) = measure(
        || {
            document
                .search(page, "the".into(), false)
                .unwrap()
                .len()
        },
        iterations,
    );
    let (render_ns, render_sum) = measure(
        || document.render_page(page, 0.25).unwrap().bgra.len(),
        iterations,
    );
    println!("{{\"language\":\"rust\",\"iterations\":{iterations},\"pageCount\":{{\"nsPerIteration\":{page_count_ns},\"checksum\":{page_count_sum}}},\"pageSize\":{{\"nsPerIteration\":{page_size_ns},\"checksum\":{page_size_sum}}},\"pageText\":{{\"nsPerIteration\":{text_ns},\"checksum\":{text_sum}}},\"search\":{{\"nsPerIteration\":{search_ns},\"checksum\":{search_sum}}},\"render\":{{\"nsPerIteration\":{render_ns},\"checksum\":{render_sum}}}}}");
}
