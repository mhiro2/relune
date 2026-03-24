//! Geometry types and helper functions for SVG rendering.

/// A 2D point with x and y coordinates.
#[derive(Debug, Clone, Copy, Default)]
pub struct Point {
    /// The x coordinate.
    pub x: f32,
    /// The y coordinate.
    pub y: f32,
}

impl Point {
    /// Creates a new point with the given coordinates.
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Returns the distance between this point and another.
    #[must_use]
    pub fn distance_to(&self, other: &Self) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        dx.hypot(dy)
    }
}

/// A rectangle defined by position (x, y) and dimensions (width, height).
#[derive(Debug, Clone, Copy, Default)]
pub struct Rect {
    /// The x coordinate of the top-left corner.
    pub x: f32,
    /// The y coordinate of the top-left corner.
    pub y: f32,
    /// The width of the rectangle.
    pub width: f32,
    /// The height of the rectangle.
    pub height: f32,
}

impl Rect {
    /// Creates a new rectangle with the given position and dimensions.
    #[must_use]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Creates a rectangle from two corner points.
    #[must_use]
    pub fn from_points(p1: &Point, p2: &Point) -> Self {
        let x = p1.x.min(p2.x);
        let y = p1.y.min(p2.y);
        let width = (p2.x - p1.x).abs();
        let height = (p2.y - p1.y).abs();
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Returns the x coordinate of the right edge.
    #[must_use]
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    /// Returns the y coordinate of the bottom edge.
    #[must_use]
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// Returns the center point of the rectangle.
    #[must_use]
    pub fn center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    /// Returns the center point of the top edge.
    #[must_use]
    pub fn top_center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y)
    }

    /// Returns the center point of the bottom edge.
    #[must_use]
    pub fn bottom_center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height)
    }

    /// Returns the center point of the left edge.
    #[must_use]
    pub fn left_center(&self) -> Point {
        Point::new(self.x, self.y + self.height / 2.0)
    }

    /// Returns the center point of the right edge.
    #[must_use]
    pub fn right_center(&self) -> Point {
        Point::new(self.x + self.width, self.y + self.height / 2.0)
    }

    /// Checks if this rectangle contains the given point.
    #[must_use]
    pub fn contains_point(&self, point: &Point) -> bool {
        point.x >= self.x
            && point.x <= self.right()
            && point.y >= self.y
            && point.y <= self.bottom()
    }

    /// Checks if this rectangle intersects with another.
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }

    /// Returns a new rectangle with expanded margins.
    #[must_use]
    pub fn expand(&self, margin: f32) -> Self {
        Self::new(
            self.x - margin,
            self.y - margin,
            margin.mul_add(2.0, self.width),
            margin.mul_add(2.0, self.height),
        )
    }
}

/// Computes the total height of a node based on the number of columns.
///
/// # Arguments
/// * `column_count` - The number of columns in the node
/// * `column_height` - The height of each column row
///
/// # Returns
/// The total height including header (42px) and bottom padding (16px)
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn compute_node_height(column_count: usize, column_height: f32) -> f32 {
    (column_count as f32).mul_add(column_height, 42.0) + 16.0
}

/// Computes the y position for a column at the given index.
///
/// # Arguments
/// * `start_y` - The starting y position (typically node.y + `header_height`)
/// * `index` - The zero-based column index
/// * `column_height` - The height of each column row
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub const fn compute_column_y(start_y: f32, index: usize, column_height: f32) -> f32 {
    start_y + (index as f32) * column_height
}

/// Interpolates between two values.
///
/// # Arguments
/// * `a` - The start value
/// * `b` - The end value
/// * `t` - The interpolation factor (0.0 to 1.0)
#[must_use]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    (b - a).mul_add(t.clamp(0.0, 1.0), a)
}

/// Clamps a value within a range.
#[must_use]
pub const fn clamp(value: f32, min: f32, max: f32) -> f32 {
    value.clamp(min, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_distance() {
        let p1 = Point::new(0.0, 0.0);
        let p2 = Point::new(3.0, 4.0);
        assert!((p1.distance_to(&p2) - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_rect_center() {
        let rect = Rect::new(0.0, 0.0, 100.0, 50.0);
        let center = rect.center();
        assert!((center.x - 50.0).abs() < 0.001);
        assert!((center.y - 25.0).abs() < 0.001);
    }

    #[test]
    fn test_rect_contains_point() {
        let rect = Rect::new(10.0, 10.0, 100.0, 50.0);
        assert!(rect.contains_point(&Point::new(50.0, 30.0)));
        assert!(!rect.contains_point(&Point::new(5.0, 30.0)));
    }

    #[test]
    fn test_compute_node_height() {
        // Header: 42px, 5 columns * 18px = 90px, bottom padding: 16px
        let height = compute_node_height(5, 18.0);
        assert!((height - 148.0).abs() < 0.001);
    }
}
