//! Raster and vector stroke rendering, page templates.
//!
//! Both backends share the stroke model from `folio-nmdl` and kfxlib's path
//! splitting ([`geom::stroke_paths`]). [`raster`] draws 8-bit grey images
//! (pencil texture or solid grey); [`vector`] emits PDF path operators.

pub mod geom;
pub mod raster;
mod rng;
pub mod vector;

pub use raster::{GrayImage, PencilMode, RasterOptions, render_page, render_template};
pub use vector::{VectorPage, template_content, vector_page};

/// Bump when rendering output changes, so caches keyed on it are invalidated.
pub const RENDERER_VERSION: u32 = 2;
