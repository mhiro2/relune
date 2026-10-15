//! Command implementations for relune CLI.

pub mod diff;
pub mod doc;
pub mod export;
mod input;
pub mod inspect;
pub mod lint;
pub mod render;
pub mod review;

pub use diff::run_diff;
pub use doc::run_doc;
pub use export::run_export;
pub use inspect::run_inspect;
pub use lint::run_lint;
pub use render::run_render;
pub use review::run_review;

use crate::cli::{DensityArg, DirectionArg, EdgeStyleArg, LayoutAlgorithmArg};
use relune_app::LayoutSpec;

/// Layout settings shared by the commands that position a diagram.
fn layout_spec(
    algorithm: LayoutAlgorithmArg,
    edge_style: EdgeStyleArg,
    direction: DirectionArg,
    density: Option<DensityArg>,
) -> LayoutSpec {
    LayoutSpec {
        algorithm: algorithm.into(),
        edge_style: edge_style.into(),
        direction: direction.into(),
        density: density.map(Into::into),
        ..Default::default()
    }
}
