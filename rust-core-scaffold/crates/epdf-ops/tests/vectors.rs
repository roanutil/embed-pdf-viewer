use test_support::{fixture_bytes, max_abs_diff, mean_abs_diff, vectors};

#[test]
fn the_six_operations_match_the_frozen_vectors() {
    let v = vectors();
    let bytes = fixture_bytes();

    let engine = epdf_ops::Engine::new().unwrap();
    let doc = engine.open(bytes, None).unwrap();

    assert_eq!(doc.page_count().unwrap(), v.page_count);

    let size = doc.page_size(v.page_index).unwrap();
    assert!((size.width - v.width).abs() < 0.001);
    assert!((size.height - v.height).abs() < 0.001);

    assert_eq!(doc.page_text(v.page_index).unwrap(), v.text);

    let hits = doc
        .search(v.page_index, v.search_query.clone(), v.search_case_sensitive)
        .unwrap();
    assert_eq!(hits.len(), v.search_hit_count);
    for (got, &(char_index, char_count)) in hits.iter().zip(&v.search_first_hits) {
        assert_eq!(got.char_index, char_index);
        assert_eq!(got.char_count, char_count);
    }

    let bitmap = doc.render_page(v.page_index, v.render_scale).unwrap();
    assert_eq!(bitmap.width, v.render_width);
    assert_eq!(bitmap.height, v.render_height);
    assert_eq!(bitmap.stride, v.render_stride);
    let mean = mean_abs_diff(&bitmap.bgra, &v.golden);
    assert!(mean <= v.tolerance, "mean abs diff {mean} exceeds {}", v.tolerance);
    let worst = max_abs_diff(&bitmap.bgra, &v.golden);
    assert!(worst <= v.max_tolerance, "max abs diff {worst} exceeds {}", v.max_tolerance);

    doc.close();
    doc.close(); // idempotent
    assert_eq!(doc.page_count(), Err(epdf_ops::OpsError::Closed));
}

#[test]
fn a_page_past_the_end_is_an_error_not_a_panic() {
    let bytes = fixture_bytes();
    let engine = epdf_ops::Engine::new().unwrap();
    let doc = engine.open(bytes, None).unwrap();
    let index = doc.page_count().unwrap();
    assert_eq!(doc.page_size(index), Err(epdf_ops::OpsError::PageOutOfRange { index }));
}

#[test]
fn garbage_bytes_are_a_load_failure() {
    let engine = epdf_ops::Engine::new().unwrap();
    assert_eq!(engine.open(b"not a pdf".to_vec(), None).err(), Some(epdf_ops::OpsError::LoadFailed));
}
