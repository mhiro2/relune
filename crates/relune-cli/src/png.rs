//! SVG-to-PNG rasterization using resvg.

use anyhow::{Context, Result};

/// Rasterize an SVG string to PNG bytes.
pub fn svg_to_png(svg: &str) -> Result<Vec<u8>> {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();

    let tree = resvg::usvg::Tree::from_str(svg, &options)
        .context("Failed to parse SVG for rasterization")?;

    let size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height())
        .context("Failed to create pixmap (image dimensions may be zero)")?;

    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );

    let png_data = pixmap.encode_png().context("Failed to encode PNG")?;

    Ok(png_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_simple_svg_to_png() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
            <rect width="100" height="100" fill="red"/>
        </svg>"#;

        let png_data = svg_to_png(svg).expect("should convert SVG to PNG");

        // PNG magic bytes
        assert_eq!(&png_data[..8], b"\x89PNG\r\n\x1a\n");
        assert!(png_data.len() > 100, "PNG should have reasonable size");
    }

    #[test]
    fn rejects_invalid_svg() {
        let result = svg_to_png("not valid svg");
        assert!(result.is_err());
    }

    #[test]
    fn handles_svg_with_text() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="50">
            <text x="10" y="30" font-size="20">Hello</text>
        </svg>"#;

        let png_data = svg_to_png(svg).expect("should handle SVG with text elements");
        assert_eq!(&png_data[..8], b"\x89PNG\r\n\x1a\n");
    }
}
