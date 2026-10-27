//! `icons.ts`: the note and file-attachment icons, drawn live as the engine
//! bakes them. Each is designed in a 20 x 20 square, y up, stretched onto
//! the annotation's box.

use serde_json::Value;

use crate::js;
use crate::kinds::blend_for;
use crate::types::{Paint, Point, Rect, SceneNode, Style};

const DESIGN: f64 = 20.0;
const OUTLINE: f64 = 1.0;
const KAPPA: f64 = 0.5523;

fn num(value: f64) -> f64 {
    js::fixed(value, 3)
}

/// Builds an SVG path from design coordinates.
struct DesignPath<'a> {
    parts: Vec<String>,
    to_page: &'a dyn Fn(f64, f64) -> Point,
}

impl<'a> DesignPath<'a> {
    fn new(to_page: &'a dyn Fn(f64, f64) -> Point) -> Self {
        DesignPath { parts: Vec::new(), to_page }
    }
    fn point(&self, x: f64, y: f64) -> String {
        let p = (self.to_page)(x, y);
        format!("{} {}", js::number_to_string(num(p.x)), js::number_to_string(num(p.y)))
    }
    fn mv(mut self, x: f64, y: f64) -> Self {
        let p = self.point(x, y);
        self.parts.push(format!("M {p}"));
        self
    }
    fn line(mut self, x: f64, y: f64) -> Self {
        let p = self.point(x, y);
        self.parts.push(format!("L {p}"));
        self
    }
    fn curve(mut self, x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) -> Self {
        let (a, b, c) = (self.point(x1, y1), self.point(x2, y2), self.point(x3, y3));
        self.parts.push(format!("C {a} {b} {c}"));
        self
    }
    fn close(mut self) -> Self {
        self.parts.push("Z".into());
        self
    }
    fn rect(self, x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        self.mv(x1, y1).line(x2, y1).line(x2, y2).line(x1, y2).close()
    }
    fn ellipse(self, x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        let mx = (x1 + x2) / 2.0;
        let my = (y1 + y2) / 2.0;
        let dx = (KAPPA * (x2 - x1).abs()) / 2.0;
        let dy = (KAPPA * (y2 - y1).abs()) / 2.0;
        let (left, right) = (js::min(x1, x2), js::max(x1, x2));
        let (bottom, top) = (js::min(y1, y2), js::max(y1, y2));
        self.mv(mx, top)
            .curve(mx + dx, top, right, my + dy, right, my)
            .curve(right, my - dy, mx + dx, bottom, mx, bottom)
            .curve(mx - dx, bottom, left, my - dy, left, my)
            .curve(left, my + dy, mx - dx, top, mx, top)
            .close()
    }
    fn d(&self) -> String {
        self.parts.join(" ")
    }
}

/// `contrastOf(color)`: black on a light colour, white on a dark one.
pub fn contrast_of(color: Option<&str>) -> &'static str {
    let rgb = color.filter(|c| {
        c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|h| h.is_ascii_hexdigit())
    });
    let channel = |i: usize| {
        rgb.map_or(0.0, |c| f64::from(u8::from_str_radix(&c[i..i + 2], 16).unwrap_or(0)) / 255.0)
    };
    let luminance = 0.299 * channel(1) + 0.587 * channel(3) + 0.114 * channel(5);
    if luminance < 0.45 { "#ffffff" } else { "#000000" }
}

struct Paints {
    contrast: &'static str,
    color: Option<Value>,
    opacity: Option<Value>,
    blend: Option<String>,
    pen: f64,
}

impl Paints {
    fn new(style: &Style, pen: f64) -> Self {
        Paints {
            contrast: contrast_of(style.color_str()),
            color: style.color.clone().filter(|c| !c.is_null()),
            opacity: style.opacity.clone(),
            blend: blend_for(style),
            pen,
        }
    }
    fn base(&self) -> Paint {
        Paint { opacity: self.opacity.clone(), blend: self.blend.clone(), ..Paint::default() }
    }
    fn body(&self, even_odd: bool) -> Paint {
        Paint {
            fill: self.color.clone(),
            stroke: Some(Value::String(self.contrast.into())),
            width: Some(OUTLINE * self.pen),
            fill_rule: even_odd.then(|| "evenodd".into()),
            ..self.base()
        }
        .ordered(&["opacity", "blend", "fill", "stroke", "width", "fillRule"])
    }
    fn outline(&self, width: f64, round: bool) -> Paint {
        Paint {
            stroke: Some(Value::String(self.contrast.into())),
            width: Some(width * self.pen),
            line_cap: round.then(|| "round".into()),
            join: round.then(|| "round".into()),
            ..self.base()
        }
        .ordered(&["opacity", "blend", "stroke", "width", "lineCap", "join"])
    }
    fn mark(&self) -> Paint {
        Paint { fill: Some(Value::String(self.contrast.into())), ..self.base() }
            .ordered(&["opacity", "blend", "fill"])
    }
    fn wire(&self, width: f64) -> Paint {
        Paint {
            stroke: self.color.clone(),
            width: Some(width * self.pen),
            line_cap: Some("round".into()),
            join: Some("round".into()),
            ..self.base()
        }
        .ordered(&["opacity", "blend", "stroke", "width", "lineCap", "join"])
    }
}

fn design_onto(rect: Rect) -> (impl Fn(f64, f64) -> Point, f64) {
    let sx = rect.width / DESIGN;
    let sy = rect.height / DESIGN;
    (
        move |x: f64, y: f64| Point { x: rect.x + x * sx, y: rect.y + (DESIGN - y) * sy },
        (sx * sy).sqrt(),
    )
}

fn node(path: &DesignPath, paint: Paint) -> SceneNode {
    SceneNode::path(path.d(), paint)
}

/// `noteIconScene(icon, box, style)`: a note's icon (`note` when it has none).
pub fn note_icon_scene(icon: Option<&str>, rect: Rect, style: &Style) -> Vec<SceneNode> {
    let (to_page, pen) = design_onto(rect);
    let paints = Paints::new(style, pen);
    let inset = |x: f64, y: f64| {
        to_page(
            OUTLINE / 2.0 + (x / DESIGN) * (DESIGN - OUTLINE),
            OUTLINE / 2.0 + (y / DESIGN) * (DESIGN - OUTLINE),
        )
    };
    let path = || DesignPath::new(&to_page);
    let glyph = || DesignPath::new(&inset);
    match icon {
        Some("comment") => {
            let mut balloon = path()
                .mv(0.5, 4.5)
                .line(0.5, 19.5)
                .line(19.5, 19.5)
                .line(19.5, 4.5)
                .line(8.5, 4.5)
                .line(6.5, 0.5)
                .line(4.5, 4.5)
                .line(0.5, 4.5);
            for y in [15.75, 12.0, 8.25] {
                balloon = balloon.mv(2.5, y).line(17.5, y);
            }
            vec![node(&balloon, paints.body(true))]
        }
        Some("help") => {
            let question = glyph()
                .mv(7.2, 12.8)
                .curve(7.2, 15.2, 8.6, 16.2, 10.2, 16.2)
                .curve(11.9, 16.2, 13.0, 15.1, 13.0, 13.6)
                .curve(13.0, 11.6, 10.2, 11.3, 10.2, 9.0)
                .line(10.2, 7.8);
            vec![
                node(&glyph().ellipse(1.0, 1.0, 19.0, 19.0), paints.body(false)),
                node(&question, paints.outline(1.8, true)),
                node(&glyph().ellipse(9.0, 4.0, 11.4, 6.4), paints.mark()),
            ]
        }
        Some("key") => {
            let key = glyph()
                .ellipse(1.5, 8.5, 10.5, 17.5)
                .ellipse(4.2, 11.2, 7.8, 14.8)
                .rect(10.5, 11.8, 18.5, 14.2)
                .rect(13.5, 8.8, 15.0, 11.8)
                .rect(16.5, 9.8, 18.5, 11.8);
            vec![node(&key, paints.body(true))]
        }
        Some("new-paragraph") => {
            let mark = glyph()
                .mv(3.0, 8.0)
                .line(17.0, 8.0)
                .line(10.0, 18.0)
                .close()
                .rect(3.0, 2.0, 17.0, 5.0);
            vec![node(&mark, paints.body(true))]
        }
        Some("paragraph") => {
            let pilcrow = glyph()
                .mv(15.5, 18.0)
                .line(15.5, 2.0)
                .line(13.7, 2.0)
                .line(13.7, 16.2)
                .line(11.8, 16.2)
                .line(11.8, 2.0)
                .line(10.0, 2.0)
                .line(10.0, 9.6)
                .curve(6.6, 9.6, 4.0, 11.4, 4.0, 13.8)
                .curve(4.0, 16.2, 6.6, 18.0, 10.0, 18.0)
                .close();
            vec![node(&pilcrow, paints.body(false))]
        }
        Some("insert") => {
            let caret = glyph()
                .mv(2.5, 3.0)
                .line(10.0, 17.0)
                .line(17.5, 3.0)
                .line(14.3, 3.0)
                .line(10.0, 11.2)
                .line(5.7, 3.0)
                .close();
            vec![node(&caret, paints.body(false))]
        }
        _ => {
            let page = glyph()
                .mv(3.5, 1.0)
                .line(3.5, 19.0)
                .line(12.5, 19.0)
                .line(16.5, 15.0)
                .line(16.5, 1.0)
                .close();
            let mut fold = glyph().mv(12.5, 19.0).line(12.5, 15.0).line(16.5, 15.0);
            for y in [12.0, 8.5, 5.0] {
                fold = fold.mv(6.0, y).line(14.0, y);
            }
            vec![node(&page, paints.body(false)), node(&fold, paints.outline(OUTLINE, false))]
        }
    }
}

/// `fileAttachmentIconScene(icon, box, style)`: `push-pin` when it has none.
pub fn file_attachment_icon_scene(icon: Option<&str>, rect: Rect, style: &Style) -> Vec<SceneNode> {
    let (to_page, pen) = design_onto(rect);
    let paints = Paints::new(style, pen);
    let inset = |x: f64, y: f64| {
        to_page(OUTLINE / 2.0 + x * (DESIGN - OUTLINE), OUTLINE / 2.0 + y * (DESIGN - OUTLINE))
    };
    let glyph = || DesignPath::new(&inset);
    match icon {
        Some("paperclip") => {
            let wire = glyph()
                .mv(0.32, 0.28)
                .line(0.32, 0.72)
                .curve(0.32, 0.85, 0.68, 0.85, 0.68, 0.72)
                .line(0.68, 0.2)
                .curve(0.68, 0.1, 0.5, 0.1, 0.5, 0.2)
                .line(0.5, 0.65)
                .curve(0.5, 0.72, 0.41, 0.72, 0.41, 0.65)
                .line(0.41, 0.3);
            vec![node(&wire, paints.outline(2.6, true)), node(&wire, paints.wire(1.4))]
        }
        Some("graph") => {
            let graph = glyph()
                .rect(0.1, 0.1, 0.9, 0.9)
                .rect(0.16, 0.16, 0.84, 0.84)
                .rect(0.22, 0.16, 0.36, 0.4)
                .rect(0.43, 0.16, 0.57, 0.56)
                .rect(0.64, 0.16, 0.78, 0.76);
            vec![node(&graph, paints.body(true))]
        }
        Some("tag") => {
            let tag = glyph()
                .mv(0.1, 0.5)
                .line(0.34, 0.8)
                .line(0.9, 0.8)
                .line(0.9, 0.2)
                .line(0.34, 0.2)
                .close()
                .ellipse(0.305, 0.445, 0.415, 0.555);
            vec![node(&tag, paints.body(true))]
        }
        _ => {
            let pin = glyph()
                .ellipse(0.33, 0.53, 0.67, 0.87)
                .rect(0.37, 0.46, 0.63, 0.525)
                .mv(0.47, 0.455)
                .line(0.53, 0.455)
                .line(0.5, 0.1)
                .close();
            vec![node(&pin, paints.body(false))]
        }
    }
}
