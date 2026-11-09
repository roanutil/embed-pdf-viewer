use epdf_ffi::{CancelToken, Rect};
use epdf_plugin_render::ffi::{RenderConfig, RenderPageOptions, RenderRotation, RenderedImage};
use epdf_viewer_host::{ViewerConfig, ViewerHost};
use futures::executor::block_on;
use std::sync::Arc;
mod common;

fn pixel(image: &RenderedImage, x: usize, y: usize) -> &[u8] {
    let offset = y * image.stride as usize + x * 4;
    &image.pixels[offset..offset + 4]
}

#[test]
fn native_crops_and_absolute_rotations_preserve_orientation_and_scale() {
    let host = ViewerHost::new(
        ViewerConfig {
            render: Some(RenderConfig::default()),
            page_edit: true,
            ..Default::default()
        },
        Arc::new(common::NullDelegate),
    )
    .unwrap();
    block_on(host.start()).unwrap();
    let doc = block_on(
        host.open_document(test_support::fixture(test_support::COLORS), common::options()),
    )
    .unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let render = block_on(host.render(doc.id.clone())).unwrap();
    // An intrinsic /Rotate must not compose with the requested absolute rotation.
    let edit = block_on(host.page_edit(doc.id.clone())).unwrap();
    block_on(edit.set_rotation(
        vec![page],
        epdf_plugin_page_edit::ffi::PageEditRotation::Clockwise90,
        CancelToken::new(),
    ))
    .unwrap();
    for (rotation, width, height, green, blue) in [
        (RenderRotation::None, 80, 40, (20, 20), (60, 20)),
        (RenderRotation::Clockwise90, 40, 80, (20, 20), (20, 60)),
        (RenderRotation::Clockwise180, 80, 40, (60, 20), (20, 20)),
        (RenderRotation::Clockwise270, 40, 80, (20, 60), (20, 20)),
    ] {
        let options = RenderPageOptions {
            rect: Some(Rect { x: 20.0, y: 0.0, width: 40.0, height: 20.0 }),
            rotation: Some(rotation),
            scale: Some(2.0),
            ..Default::default()
        };
        let image = block_on(render.render_page(page, options, CancelToken::new())).unwrap();
        assert_eq!((image.width, image.height), (width, height));
        assert_eq!(image.scale, 2.0);
        assert_eq!(pixel(&image, green.0, green.1), [0, 255, 0, 255]);
        assert_eq!(pixel(&image, blue.0, blue.1), [0, 0, 255, 255]);
        let batch =
            block_on(render.render_pages(vec![page], options, None, CancelToken::new())).unwrap();
        assert_eq!(batch.applied[0].image, image);
    }
    let clipped = block_on(render.render_page(
        page,
        RenderPageOptions {
            rect: Some(Rect { x: 60.0, y: 0.0, width: 40.0, height: 20.0 }),
            rotation: Some(RenderRotation::Clockwise90),
            width: Some(60.0),
            ..Default::default()
        },
        CancelToken::new(),
    ))
    .unwrap();
    assert_eq!((clipped.width, clipped.height, clipped.scale), (60, 60, 3.0));
    block_on(host.close_document(doc.id)).unwrap();
    assert_eq!(pixel(&clipped, 30, 30), [51, 102, 153, 255]);
}

#[test]
fn native_snapshot_round_trip_is_document_scoped_and_owns_its_pixels() {
    use epdf_ffi::{Modifiers, Point, PointerPageHit, PointerPhase, PointerSample};
    let host = ViewerHost::new(
        ViewerConfig {
            render: Some(RenderConfig::default()),
            interaction: true,
            ..Default::default()
        },
        Arc::new(common::NullDelegate),
    )
    .unwrap();
    block_on(host.start()).unwrap();
    let doc = block_on(
        host.open_document(test_support::fixture(test_support::COLORS), common::options()),
    )
    .unwrap();
    let other_doc = block_on(
        host.open_document(test_support::fixture(test_support::COLORS), common::options()),
    )
    .unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let render = block_on(host.render(doc.id.clone())).unwrap();
    let other = block_on(host.render(other_doc.id.clone())).unwrap();
    let interaction = block_on(host.interaction(doc.id.clone())).unwrap();
    block_on(interaction.activate_tool("snapshot".into())).unwrap();
    for (phase, point) in [
        (PointerPhase::Down, Point { x: 20.0, y: 0.0 }),
        (PointerPhase::Up, Point { x: 60.0, y: 20.0 }),
    ] {
        interaction.dispatch_pointer(PointerSample {
            phase,
            viewport: point,
            page: Some(PointerPageHit {
                page,
                point,
                scale: Some(1.0),
                rotation: Some(90.0),
                zoom: None,
            }),
            modifiers: Modifiers::default(),
            click_count: None,
            pointer_type: None,
            gesture: None,
            source: None,
        });
    }
    let snapshot = block_on(render.snapshot()).unwrap().unwrap();
    assert_eq!(snapshot.rect, Rect { x: 20.0, y: 0.0, width: 40.0, height: 20.0 });
    assert!(block_on(other.snapshot()).unwrap().is_none());
    let second = block_on(render.snapshot()).unwrap().unwrap();
    let image = block_on(snapshot.image.image(CancelToken::new())).unwrap();
    assert_eq!(image, block_on(second.image.image(CancelToken::new())).unwrap());
    assert_eq!((image.width, image.height, image.scale), (40, 80, 2.0));
    block_on(other.clear_snapshot()).unwrap();
    assert!(block_on(render.snapshot()).unwrap().is_some());
    block_on(render.clear_snapshot()).unwrap();
    block_on(host.close_document(doc.id)).unwrap();
    assert!(block_on(other.can_snapshot()).unwrap());
    block_on(host.destroy()).unwrap();
    assert_eq!(pixel(&image, 20, 20), [0, 255, 0, 255]);
    assert_eq!(pixel(&image, 20, 60), [0, 0, 255, 255]);
}
