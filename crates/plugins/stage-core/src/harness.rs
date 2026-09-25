//! The conformance adapter: the function table the vectors call by the
//! TypeScript names, with arguments and results in canonical JSON. The
//! scene and anchor entries mirror the wrappers in
//! `tooling/plugin-conformance/src/suites/stage.ts`.

use epdf_kernel::Error;
use plugin_conformance::FunctionTable;
use plugin_conformance::js::{arg, to_value};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::*;

type Out = Result<Value, Error>;

fn ok<T: Serialize>(value: T) -> Out {
    Ok(to_value(&value))
}

fn thrown(e: StageError) -> Error {
    Error::named(e.name(), e.to_string())
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Probe {
    #[serde(default)]
    rects: Option<Vec<Rect>>,
    #[serde(default)]
    points: Option<Vec<Point>>,
    #[serde(default)]
    page_indexes: Option<Vec<usize>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SceneProbe<'a> {
    #[serde(flatten)]
    scene: &'a Scene,
    query: Vec<Vec<usize>>,
    nearest_item: Vec<Option<usize>>,
    item_of_page: Vec<isize>,
}

fn scene_probe(scene: &Scene, probe: Option<Probe>) -> Out {
    let probe = probe.unwrap_or_default();
    ok(SceneProbe {
        scene,
        query: probe
            .rects
            .unwrap_or_default()
            .into_iter()
            .map(|r| scene.query(r).iter().map(|it| it.index).collect())
            .collect(),
        nearest_item: probe
            .points
            .unwrap_or_default()
            .into_iter()
            .map(|p| scene.nearest_item(p).map(|it| it.index))
            .collect(),
        item_of_page: probe
            .page_indexes
            .unwrap_or_default()
            .into_iter()
            .map(|pi| scene.item_of_page(pi).map_or(-1, |i| i as isize))
            .collect(),
    })
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Layout {
    Linear {
        pages: Vec<PageGeom>,
        grouping: Vec<Vec<usize>>,
        #[serde(default)]
        opts: Option<LinearOptions>,
    },
    Grid {
        pages: Vec<PageGeom>,
        grouping: Vec<Vec<usize>>,
        #[serde(default)]
        opts: Option<GridOptions>,
    },
}

fn build(layout: Layout) -> Result<Scene, Error> {
    match layout {
        Layout::Linear { pages, grouping, opts } => {
            linear_layout(&pages, &grouping, opts.unwrap_or_default())
        }
        Layout::Grid { pages, grouping, opts } => {
            grid_layout(&pages, &grouping, opts.unwrap_or_default())
        }
    }
    .map_err(thrown)
}

fn padding(a: &[Value], i: usize) -> Result<f64, Error> {
    Ok(arg::<Option<f64>>(a, i)?.unwrap_or(0.0))
}

pub fn functions() -> FunctionTable {
    vec![
        ("constants", |_| {
            Ok(json!({
                "ZOOM_MIN": ZOOM_MIN,
                "ZOOM_MAX": ZOOM_MAX,
                "ZoomMode": {
                    "Automatic": to_value(&ZoomMode::Automatic),
                    "FitPage": to_value(&ZoomMode::FitPage),
                    "FitWidth": to_value(&ZoomMode::FitWidth),
                    "FitAll": to_value(&ZoomMode::FitAll),
                },
            }))
        }),
        ("toScreen", |a| ok(to_screen(arg(a, 0)?, arg(a, 1)?))),
        ("toWorld", |a| ok(to_world(arg(a, 0)?, arg(a, 1)?))),
        ("cameraWorldRect", |a| ok(camera_world_rect(arg(a, 0)?, arg(a, 1)?))),
        ("zoomAround", |a| ok(zoom_around(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?))),
        ("panByScreen", |a| ok(pan_by_screen(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?))),
        ("centerOnWorld", |a| ok(center_on_world(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?))),
        ("travelRange", |a| {
            ok(travel_range(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?, arg(a, 3)?, arg(a, 4)?))
        }),
        ("clampCamera", |a| ok(clamp_camera(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?, arg(a, 3)?))),
        ("scrollMetrics", |a| ok(scroll_metrics(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?, arg(a, 3)?))),
        ("cameraFromScroll", |a| {
            ok(camera_from_scroll(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?, arg(a, 3)?, arg(a, 4)?))
        }),
        ("placeCamera", |a| {
            ok(place_camera(
                arg(a, 0)?,
                arg(a, 1)?,
                arg(a, 2)?,
                padding(a, 3)?,
                arg::<Option<AlignmentValue>>(a, 4)?.unwrap_or_default(),
                arg::<Option<Direction>>(a, 5)?.unwrap_or_default(),
            ))
        }),
        ("revealCamera", |a| ok(reveal_camera(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?, padding(a, 3)?))),
        ("resolveZoom", |a| ok(resolve_zoom(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?, padding(a, 3)?))),
        ("groupPages", |a| {
            ok(group_pages(arg(a, 0)?, arg::<Option<SpreadMode>>(a, 1)?.unwrap_or_default()))
        }),
        ("linearLayout", |a| {
            let opts: Option<LinearOptions> = arg(a, 2)?;
            let scene = linear_layout(
                &arg::<Vec<PageGeom>>(a, 0)?,
                &arg::<Vec<Vec<usize>>>(a, 1)?,
                opts.unwrap_or_default(),
            )
            .map_err(thrown)?;
            scene_probe(&scene, arg(a, 3)?)
        }),
        ("gridLayout", |a| {
            let opts: Option<GridOptions> = arg(a, 2)?;
            let scene = grid_layout(
                &arg::<Vec<PageGeom>>(a, 0)?,
                &arg::<Vec<Vec<usize>>>(a, 1)?,
                opts.unwrap_or_default(),
            )
            .map_err(thrown)?;
            scene_probe(&scene, arg(a, 3)?)
        }),
        ("anchorAtPoint", |a| {
            let scene = build(arg(a, 0)?)?;
            ok(anchor_at_point(&scene, arg(a, 1)?).map_err(thrown)?)
        }),
        ("cameraForAnchorAtScreen", |a| {
            let scene = build(arg(a, 1)?)?;
            ok(camera_for_anchor_at_screen(arg(a, 0)?, &scene, arg(a, 2)?, arg(a, 3)?)
                .map_err(thrown)?)
        }),
        ("anchorFromCamera", |a| {
            let scene = build(arg(a, 1)?)?;
            ok(anchor_from_camera(arg(a, 0)?, &scene, arg(a, 2)?, arg(a, 3)?).map_err(thrown)?)
        }),
        ("cameraFromAnchor", |a| {
            let scene = build(arg(a, 1)?)?;
            ok(camera_from_anchor(arg(a, 0)?, &scene, arg(a, 2)?, arg(a, 3)?, arg(a, 4)?)
                .map_err(thrown)?)
        }),
    ]
}
