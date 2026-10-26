//! SVG-to-PNG rasterization using resvg.

use anyhow::{Context, Result, anyhow, bail};

/// Maximum number of pixels we will rasterize into a PNG.
///
/// The pixmap is RGBA8, so this 100-megapixel ceiling caps the allocation at
/// roughly 400 MiB and prevents a huge diagram from triggering an OOM (or an
/// opaque allocation failure) during rasterization. Large schemas should be
/// rendered to SVG instead.
const MAX_IMAGE_PIXELS: u64 = 100_000_000;

/// Rasterize an SVG string to PNG bytes.
pub fn svg_to_png(svg: &str) -> Result<Vec<u8>> {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();

    let tree = resvg::usvg::Tree::from_str(svg, &options)
        .context("Failed to parse SVG for rasterization")?;

    let size = tree.size().to_int_size();
    check_image_dimensions(size.width(), size.height())?;

    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(size.width(), size.height()).ok_or_else(|| {
            anyhow!(
                "Failed to allocate {}x{} pixmap",
                size.width(),
                size.height()
            )
        })?;

    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );

    let png_data = pixmap.encode_png().context("Failed to encode PNG")?;

    Ok(png_data)
}

/// Validate the rasterization target size before allocating the pixmap.
fn check_image_dimensions(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 {
        bail!(
            "Diagram has zero {} and cannot be rasterized to PNG",
            if width == 0 { "width" } else { "height" }
        );
    }

    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_IMAGE_PIXELS {
        bail!(
            "Diagram is too large to rasterize to PNG: {width}x{height} = {pixels} pixels exceeds the {MAX_IMAGE_PIXELS} pixel limit. Render to SVG instead, or narrow the diagram with --focus/--include/--exclude."
        );
    }

    Ok(())
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
    fn accepts_dimensions_within_budget() {
        check_image_dimensions(1920, 1080).expect("reasonable dimensions should be accepted");
    }

    #[test]
    fn rejects_zero_dimensions() {
        let error = check_image_dimensions(0, 100).expect_err("zero width should be rejected");
        assert!(error.to_string().contains("zero width"), "error: {error}");

        let error = check_image_dimensions(100, 0).expect_err("zero height should be rejected");
        assert!(error.to_string().contains("zero height"), "error: {error}");
    }

    #[test]
    fn rejects_dimensions_over_budget() {
        // 20000 * 20000 = 400M pixels, well over the 100M ceiling.
        let error =
            check_image_dimensions(20_000, 20_000).expect_err("oversized image should be rejected");
        let message = error.to_string();
        assert!(message.contains("too large"), "error: {message}");
        assert!(
            message.contains("SVG"),
            "error should suggest SVG: {message}"
        );
    }

    /// Counts pixels that are not fully transparent in a PNG.
    fn painted_pixels(png_data: &[u8]) -> usize {
        let pixmap = resvg::tiny_skia::Pixmap::decode_png(png_data).expect("decode PNG");
        pixmap
            .pixels()
            .iter()
            .filter(|pixel| pixel.alpha() > 0)
            .count()
    }

    #[test]
    fn draws_svg_text_with_system_fonts() {
        // Same font stack as the SVG renderer, so a missing font fallback or
        // skipped text pass shows up as an empty image.
        let text_svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="50">
            <text x="10" y="30" font-family="'Inter', 'Segoe UI', system-ui, sans-serif" font-size="20" fill="black">Hello</text>
        </svg>"#;
        let blank_svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="50"></svg>"#;

        let with_text = svg_to_png(text_svg).expect("should handle SVG with text elements");
        let blank = svg_to_png(blank_svg).expect("should handle an empty SVG");

        assert_eq!(painted_pixels(&blank), 0);
        // PNG export relies on the host's fonts; a host without any cannot
        // draw glyphs, which is an environment limit rather than a bug.
        let mut fonts = resvg::usvg::fontdb::Database::new();
        fonts.load_system_fonts();
        if fonts.is_empty() {
            eprintln!("no system fonts available; skipping the glyph check");
            return;
        }
        assert!(
            painted_pixels(&with_text) > 50,
            "text should paint glyph pixels"
        );
    }
}
