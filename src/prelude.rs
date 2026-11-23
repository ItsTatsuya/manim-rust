//! `use manim::prelude::*;` brings in everything a scene usually needs.

pub use crate::animation::*;
pub use crate::camera::Camera;
pub use crate::color::*;
pub use crate::config::{Config, Quality};
pub use crate::constants::*;
pub use crate::effects::{Dash, Effects, Gradient, GradientStop, Shadow};
pub use crate::image::ImageMobject;
#[cfg(feature = "math")]
pub use crate::math::{MathTex, Typst};
pub use crate::mobject::geometry::*;
pub use crate::mobject::graphing::*;
pub use crate::mobject::grid::GridMobject;
#[cfg(feature = "text")]
pub use crate::mobject::{
    code::{Code, CodeBackground},
    matrix::Matrix,
    table::Table,
};
pub use crate::mobject::{PathFunc, Subpath, VGroup, VMobject};
pub use crate::output::{Encoder, OutputFormat};
pub use crate::rate_functions::{self, *};
pub use crate::renderer::RendererKind;
pub use crate::scene::{render, render_with, Scene, UpdaterId};
pub use crate::scene::{Marker, Playback};
#[cfg(feature = "svg")]
pub use crate::svg::SVGMobject;
#[cfg(feature = "text")]
pub use crate::text::{Align, DecimalNumber, Integer, Span, Text, TextStyle, BOLD, LIGHT, MEDIUM, NORMAL, SEMIBOLD};
pub use crate::theme::{TextRole, Theme};
pub use crate::world::{MobjectId, TrackerId, World};
pub use crate::{anims, vgroup};
