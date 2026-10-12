//! Data model of the Shape meter (`G/Shape.h`).

use crate::color::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    Flat,
    Square,
    Round,
    Triangle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    Miter,
    Bevel,
    Round,
    MiterOrBevel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CombineMode {
    Union,
    Xor,
    Intersect,
    Exclude,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stop {
    pub color: Color,
    pub pos: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Color(Color),
    Linear {
        angle: f64,
        stops: Vec<Stop>,
        /// `LinearGradient1`: interpolate in linear light (gamma 1.0).
        linear_gamma: bool,
    },
    Radial {
        center: (f64, f64),
        origin: (f64, f64),
        radius: Option<(f64, f64)>,
        stops: Vec<Stop>,
        linear_gamma: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    Line { x: f64, y: f64 },
    Arc { x: f64, y: f64, rx: f64, ry: f64, angle: f64, clockwise: bool, large: bool },
    Quad { x: f64, y: f64, cx: f64, cy: f64 },
    Cubic { x: f64, y: f64, c1: (f64, f64), c2: (f64, f64) },
}

#[derive(Clone, Debug, PartialEq)]
pub struct PathSeg {
    pub seg: Seg,
    pub no_stroke: bool,
    pub round_join: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Geom {
    /// Reserved position of a `Combine` shape until it is resolved.
    None,
    Rect { x: f64, y: f64, w: f64, h: f64, rx: f64, ry: f64 },
    Ellipse { cx: f64, cy: f64, rx: f64, ry: f64 },
    Line { x1: f64, y1: f64, x2: f64, y2: f64 },
    Arc { x1: f64, y1: f64, x2: f64, y2: f64, rx: f64, ry: f64, angle: f64, clockwise: bool, large: bool, closed: bool },
    Curve { x1: f64, y1: f64, x2: f64, y2: f64, c1: (f64, f64), c2: Option<(f64, f64)>, closed: bool },
    Path { start: (f64, f64), segs: Vec<PathSeg>, closed: bool, winding: bool },
    /// Flattened polygons (result of `Combine`); drawn with the even-odd rule.
    Polys(Vec<Vec<(f64, f64)>>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformKind {
    Rotate,
    Scale,
    Skew,
    Offset,
}

pub const DEFAULT_ORDER: [TransformKind; 4] =
    [TransformKind::Rotate, TransformKind::Scale, TransformKind::Skew, TransformKind::Offset];

#[derive(Clone, Debug, PartialEq)]
pub struct Transforms {
    pub rotate: Option<(f64, Option<(f64, f64)>)>,
    pub scale: Option<((f64, f64), Option<(f64, f64)>)>,
    pub skew: Option<((f64, f64), Option<(f64, f64)>)>,
    pub offset: Option<(f64, f64)>,
    pub order: Vec<TransformKind>,
}

impl Default for Transforms {
    fn default() -> Self {
        Transforms { rotate: None, scale: None, skew: None, offset: None, order: DEFAULT_ORDER.to_vec() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShapeDef {
    pub geom: Geom,
    pub fill: Paint,
    pub stroke: Paint,
    pub stroke_width: f64,
    /// Any stroke modifier was used: the stroke is transform-FIXED.
    pub custom_stroke: bool,
    pub start_cap: Cap,
    pub end_cap: Cap,
    pub dash_cap: Cap,
    pub join: Join,
    pub miter_limit: f64,
    pub dashes: Vec<f64>,
    pub dash_offset: f64,
    pub transforms: Transforms,
    /// Taking part in a `Combine`: not drawn on its own.
    pub combined: bool,
}

impl ShapeDef {
    pub fn new(geom: Geom) -> ShapeDef {
        ShapeDef {
            geom,
            fill: Paint::Color(Color { r: 255, g: 255, b: 255, a: 255 }),
            stroke: Paint::Color(Color { r: 0, g: 0, b: 0, a: 255 }),
            stroke_width: 1.0,
            custom_stroke: false,
            start_cap: Cap::Flat,
            end_cap: Cap::Flat,
            dash_cap: Cap::Flat,
            join: Join::Miter,
            miter_limit: 10.0,
            dashes: Vec::new(),
            dash_offset: 0.0,
            transforms: Transforms::default(),
            combined: false,
        }
    }

    /// Open Arc/Curve/Path shapes get a transparent fill by default.
    pub fn transparent_fill(&mut self) {
        self.fill = Paint::Color(Color { r: 0, g: 0, b: 0, a: 0 });
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CombineItem {
    Part(CombineMode, usize),
    /// A transform modifier listed inside the Combine option.
    Modifier(String),
}

/// A `Combine ShapeN | Mode ShapeM | transform ...` description, resolved
/// after all shapes are read.
#[derive(Clone, Debug, PartialEq)]
pub struct CombineSpec {
    pub parent: usize,
    pub items: Vec<CombineItem>,
}
