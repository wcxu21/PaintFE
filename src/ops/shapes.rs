use rayon::prelude::*;

/// Available shape primitives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShapeKind {
    Ellipse,
    Rectangle,
    RoundedRect,
    Trapezoid,
    Parallelogram,
    Triangle,
    RightTriangle,
    Pentagon,
    Hexagon,
    Octagon,
    Cross,
    Check,
    Heart,
    // Legacy variants kept for compatibility but hidden from picker
    Diamond,
    Star5,
    Star6,
    Arrow,
}

impl ShapeKind {
    pub fn label(&self) -> String {
        match self {
            ShapeKind::Ellipse => t!("shape.ellipse"),
            ShapeKind::Rectangle => t!("shape.rectangle"),
            ShapeKind::RoundedRect => t!("shape.rounded_rect"),
            ShapeKind::Trapezoid => t!("shape.trapezoid"),
            ShapeKind::Parallelogram => t!("shape.parallelogram"),
            ShapeKind::Triangle => t!("shape.triangle"),
            ShapeKind::RightTriangle => t!("shape.right_triangle"),
            ShapeKind::Pentagon => t!("shape.pentagon"),
            ShapeKind::Hexagon => t!("shape.hexagon"),
            ShapeKind::Octagon => t!("shape.octagon"),
            ShapeKind::Cross => t!("shape.cross"),
            ShapeKind::Check => t!("shape.check"),
            ShapeKind::Heart => t!("shape.heart"),
            ShapeKind::Diamond => t!("shape.diamond"),
            ShapeKind::Star5 => t!("shape.star5"),
            ShapeKind::Star6 => t!("shape.star6"),
            ShapeKind::Arrow => t!("shape.arrow"),
        }
    }

    /// The icon file stem for this shape (matches the PNG filename without extension).
    pub fn icon_name(&self) -> &'static str {
        match self {
            ShapeKind::Ellipse => "ellipse",
            ShapeKind::Rectangle => "rectangle",
            ShapeKind::RoundedRect => "rounded_rect",
            ShapeKind::Trapezoid => "trapezoid",
            ShapeKind::Parallelogram => "parallelogram",
            ShapeKind::Triangle => "triangle",
            ShapeKind::RightTriangle => "right_triangle",
            ShapeKind::Pentagon => "pentagon",
            ShapeKind::Hexagon => "hexagon",
            ShapeKind::Octagon => "octagon",
            ShapeKind::Cross => "cross",
            ShapeKind::Check => "check",
            ShapeKind::Heart => "heart",
            _ => "rectangle", // legacy fallback
        }
    }

    /// Shapes shown in the picker grid (excludes legacy variants).
    pub fn picker_shapes() -> &'static [ShapeKind] {
        &[
            ShapeKind::Ellipse,
            ShapeKind::Rectangle,
            ShapeKind::RoundedRect,
            ShapeKind::Trapezoid,
            ShapeKind::Parallelogram,
            ShapeKind::Triangle,
            ShapeKind::RightTriangle,
            ShapeKind::Pentagon,
            ShapeKind::Hexagon,
            ShapeKind::Octagon,
            ShapeKind::Cross,
            ShapeKind::Check,
            ShapeKind::Heart,
        ]
    }

    /// All variants including legacy (for serialization compatibility).
    pub fn all() -> &'static [ShapeKind] {
        &[
            ShapeKind::Ellipse,
            ShapeKind::Rectangle,
            ShapeKind::RoundedRect,
            ShapeKind::Trapezoid,
            ShapeKind::Parallelogram,
            ShapeKind::Triangle,
            ShapeKind::RightTriangle,
            ShapeKind::Pentagon,
            ShapeKind::Hexagon,
            ShapeKind::Octagon,
            ShapeKind::Cross,
            ShapeKind::Check,
            ShapeKind::Heart,
            ShapeKind::Diamond,
            ShapeKind::Star5,
            ShapeKind::Star6,
            ShapeKind::Arrow,
        ]
    }
}

/// How a shape is painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeFillMode {
    Outline,
    Filled,
    Both,
}

impl ShapeFillMode {
    pub fn label(&self) -> String {
        match self {
            ShapeFillMode::Outline => t!("shape_fill.outline"),
            ShapeFillMode::Filled => t!("shape_fill.filled"),
            ShapeFillMode::Both => t!("shape_fill.both"),
        }
    }
    pub fn all() -> &'static [ShapeFillMode] {
        &[
            ShapeFillMode::Outline,
            ShapeFillMode::Filled,
            ShapeFillMode::Both,
        ]
    }
}

/// Handle for interacting with a placed shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeHandle {
    Move,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Top,
    Right,
    Bottom,
    Left,
    Rotate,
}

/// A shape that has been drawn and can be manipulated before commit.
#[derive(Clone, Debug)]
pub struct PlacedShape {
    /// Center x in canvas coords
    pub cx: f32,
    /// Center y in canvas coords
    pub cy: f32,
    /// Half-width
    pub hw: f32,
    /// Half-height
    pub hh: f32,
    /// Rotation in radians
    pub rotation: f32,
    pub kind: ShapeKind,
    pub fill_mode: ShapeFillMode,
    pub outline_width: f32,
    pub primary_color: [u8; 4],
    pub secondary_color: [u8; 4],
    pub anti_alias: bool,
    pub corner_radius: f32,
    /// Currently dragging handle
    pub handle_dragging: Option<ShapeHandle>,
    /// Offset from shape center to mouse at drag start
    pub drag_offset: [f32; 2],
    /// Anchor point (opposite corner) in canvas coords for resize
    pub drag_anchor: [f32; 2],
    /// Initial rotation for rotate handle
    pub rotate_start_angle: f32,
    pub rotate_start_rotation: f32,
}

// ============================================================================
// SDF functions — return signed distance (negative = inside)
// ============================================================================

/// SDF for a box centred at origin with half-extents (hx, hy).
#[inline]
fn sdf_box(px: f32, py: f32, hx: f32, hy: f32) -> f32 {
    let dx = px.abs() - hx;
    let dy = py.abs() - hy;
    let outside = (dx.max(0.0) * dx.max(0.0) + dy.max(0.0) * dy.max(0.0)).sqrt();
    let inside = dx.max(dy).min(0.0);
    outside + inside
}

/// SDF for a rounded box.
#[inline]
fn sdf_rounded_box(px: f32, py: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let r = r.min(hx).min(hy);
    sdf_box(px, py, hx - r, hy - r) - r
}

/// SDF for an ellipse (approximation).
#[inline]
fn sdf_ellipse(px: f32, py: f32, rx: f32, ry: f32) -> f32 {
    // Decent approximation: normalise point to circle space
    let nx = px / rx;
    let ny = py / ry;
    let len = (nx * nx + ny * ny).sqrt();
    if len < 1e-8 {
        return -rx.min(ry);
    }
    // Distance from normalised circle surface, scaled back
    let scale = (rx * rx * ny * ny + ry * ry * nx * nx).sqrt() / (rx * ry * len);
    (len - 1.0) / scale
}

/// SDF for an isosceles triangle fitted to the rectangle [-hx, hx] × [-hy, hy].
fn sdf_triangle_box(px: f32, py: f32, hx: f32, hy: f32) -> f32 {
    let ax = 0.0;
    let ay = -hy;
    let bx = hx;
    let by = hy;
    let cx = -hx;
    let cy = hy;

    let d1 = sdf_line_segment(px, py, ax, ay, bx, by);
    let d2 = sdf_line_segment(px, py, bx, by, cx, cy);
    let d3 = sdf_line_segment(px, py, cx, cy, ax, ay);
    let edge_dist = d1.min(d2.min(d3));

    let c1 = (bx - ax) * (py - ay) - (by - ay) * (px - ax);
    let c2 = (cx - bx) * (py - by) - (cy - by) * (px - bx);
    let c3 = (ax - cx) * (py - cy) - (ay - cy) * (px - cx);
    let inside = (c1 >= 0.0 && c2 >= 0.0 && c3 >= 0.0) || (c1 <= 0.0 && c2 <= 0.0 && c3 <= 0.0);

    if inside { -edge_dist } else { edge_dist }
}

/// SDF for a regular polygon with `n` sides, circumscribed radius `r`.
fn sdf_polygon(px: f32, py: f32, r: f32, n: u32) -> f32 {
    let angle = std::f32::consts::TAU / n as f32;
    let half = angle * 0.5;
    // Rotate so flat edge is on top for even-sided polygons
    let theta = py.atan2(px) + std::f32::consts::FRAC_PI_2;
    let theta = ((theta % angle) + angle) % angle - half;
    let len = (px * px + py * py).sqrt();
    let qx = len * theta.cos();
    let qy = len * theta.sin();
    let _ = qy;
    qx - r * half.cos()
}

/// SDF for a star with `n` points, outer radius `ro`, inner radius `ri`.
fn sdf_star(px: f32, py: f32, ro: f32, ri: f32, n: u32) -> f32 {
    let angle = std::f32::consts::PI / n as f32;
    let theta = py.atan2(px) + std::f32::consts::FRAC_PI_2;
    let theta = ((theta % (2.0 * angle)) + 2.0 * angle) % (2.0 * angle);

    let len = (px * px + py * py).sqrt();
    let cos_a = angle.cos();
    let sin_a = angle.sin();

    // Two edges of the star sector
    let ax = ro;
    let ay = 0.0;
    let bx = ri * cos_a;
    let by = ri * sin_a;

    let qx = len * (theta - angle).cos();
    let qy = len * (theta - angle).sin();

    // Signed distance to triangle edge (a→b)
    let ex = bx - ax;
    let ey = by - ay;
    let fx = qx - ax;
    let fy = qy - ay;
    let t = (fx * ex + fy * ey) / (ex * ex + ey * ey);
    let t = t.clamp(0.0, 1.0);
    let cx = ax + ex * t - qx;
    let cy = ay + ey * t - qy;
    let dist = (cx * cx + cy * cy).sqrt();
    let cross = ex * fy - ey * fx;
    if cross < 0.0 { -dist } else { dist }
}

/// SDF for a diamond (rotated square).
#[inline]
fn sdf_diamond(px: f32, py: f32, hx: f32, hy: f32) -> f32 {
    // Diamond is abs(x/hx) + abs(y/hy) <= 1
    let d = px.abs() / hx + py.abs() / hy - 1.0;
    let scale = 1.0 / (1.0 / (hx * hx) + 1.0 / (hy * hy)).sqrt();
    d * scale
}

/// SDF for an arrow pointing right.
fn sdf_arrow(px: f32, py: f32, hx: f32, hy: f32) -> f32 {
    // Arrow body: left half is a box, right half is a triangle
    let shaft_w = hx * 0.55;
    let shaft_h = hy * 0.35;
    let head_x = hx * 0.05; // where arrowhead starts

    if px < head_x {
        // Shaft region
        sdf_box(
            px - (-hx + shaft_w) * 0.5,
            py,
            shaft_w * 0.5 + hx * 0.25,
            shaft_h,
        )
    } else {
        // Arrowhead triangle: tip at (hx, 0), base at (head_x, +-hy)
        let tx = px - head_x;
        let tw = hx - head_x;
        let max_y = hy * (1.0 - tx / tw);
        let dy = py.abs() - max_y;
        if dy > 0.0 {
            // Outside triangle vertically
            let nx = -hy;
            let ny = tw;
            let nl = (nx * nx + ny * ny).sqrt();
            let dpx = px - hx;
            let dpy = py.abs() - 0.0;
            let to_edge = (dpx * (-hy / nl) + dpy * (tw / nl)).max(0.0);
            let to_tip = (dpx * dpx + dpy * dpy).sqrt();
            to_edge.min(to_tip)
        } else if tx > tw {
            // Past tip
            ((px - hx) * (px - hx) + py * py).sqrt()
        } else {
            // Inside
            let nx = hy;
            let ny = tw;
            let nl = (nx * nx + ny * ny).sqrt();
            -(max_y - py.abs()).min((tw - tx) * hy / nl).max(0.0)
        }
    }
}

/// Signed distance to a simple polygon path (convex or concave).
fn sdf_polygon_path(verts: &[(f32, f32)], px: f32, py: f32) -> f32 {
    let mut min_dist = f32::MAX;
    let mut inside = false;
    let mut prev = *verts.last().unwrap_or(&(0.0, 0.0));

    for &curr in verts {
        min_dist = min_dist.min(sdf_line_segment(px, py, prev.0, prev.1, curr.0, curr.1));

        let crosses_scanline = (curr.1 > py) != (prev.1 > py);
        if crosses_scanline {
            let edge_dy = prev.1 - curr.1;
            if edge_dy.abs() > f32::EPSILON {
                let edge_x = (prev.0 - curr.0) * (py - curr.1) / edge_dy + curr.0;
                if px < edge_x {
                    inside = !inside;
                }
            }
        }

        prev = curr;
    }

    if inside { -min_dist } else { min_dist }
}

/// SDF for a heart shape.
fn sdf_heart(px: f32, py: f32, hx: f32, hy: f32) -> f32 {
    // Higher-resolution parametric heart path for smoother lobes and side curves.
    let mut verts = Vec::with_capacity(96);
    let mut max_abs_x = 0.0f32;
    let mut max_abs_y = 0.0f32;
    let mut raw = Vec::with_capacity(96);
    for i in 0..96 {
        let t = i as f32 * std::f32::consts::TAU / 96.0;
        let s = t.sin();
        let c = t.cos();
        let xr = 16.0 * s * s * s;
        let yr = 13.0 * c - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos();
        max_abs_x = max_abs_x.max(xr.abs());
        max_abs_y = max_abs_y.max(yr.abs());
        raw.push((xr, yr));
    }
    let sx = if max_abs_x > 0.0 { hx * 0.98 / max_abs_x } else { 1.0 };
    let sy = if max_abs_y > 0.0 { hy * 0.98 / max_abs_y } else { 1.0 };
    for (xr, yr) in raw {
        verts.push((xr * sx, -yr * sy));
    }
    sdf_polygon_path(&verts, px, py)
}

// ---- New SDF functions for added shapes ----

/// SDF for a trapezoid (wider at bottom).
fn sdf_trapezoid(px: f32, py: f32, hx: f32, hy: f32) -> f32 {
    let top_hw = hx * 0.55;
    let verts = [(-top_hw, -hy), (top_hw, -hy), (hx, hy), (-hx, hy)];
    sdf_convex_polygon(&verts, px, py)
}

/// SDF for a parallelogram (skewed rectangle).
fn sdf_parallelogram(px: f32, py: f32, hx: f32, hy: f32) -> f32 {
    // Skew amount: shift top edge right, bottom edge left
    let skew = hx * 0.3;
    // Effective x after un-skewing
    let t = py / hy; // -1 at bottom, +1 at top (normalized)
    let shift = skew * t * 0.5;
    let ux = px - shift;
    sdf_box(ux, py, hx - skew.abs() * 0.5, hy)
}

/// SDF for a right triangle (right angle at bottom-left).
/// Vertices: bottom-left (-hx, hy), bottom-right (hx, hy), top-left (-hx, -hy).
fn sdf_right_triangle(px: f32, py: f32, hx: f32, hy: f32) -> f32 {
    // Use general polygon SDF for 3 vertices (CCW winding)
    let verts: [(f32, f32); 3] = [
        (-hx, hy),  // bottom-left  (right angle)
        (hx, hy),   // bottom-right
        (-hx, -hy), // top-left
    ];
    sdf_convex_polygon(&verts, px, py)
}

/// Signed distance to a convex polygon (vertices in CCW order).
fn sdf_convex_polygon(verts: &[(f32, f32)], px: f32, py: f32) -> f32 {
    let n = verts.len();
    let mut d = (px - verts[0].0) * (px - verts[0].0) + (py - verts[0].1) * (py - verts[0].1);
    let mut s: f32 = 1.0;
    let mut j = n - 1;
    for i in 0..n {
        let ex = verts[j].0 - verts[i].0;
        let ey = verts[j].1 - verts[i].1;
        let wx = px - verts[i].0;
        let wy = py - verts[i].1;
        let t = (wx * ex + wy * ey) / (ex * ex + ey * ey);
        let t = t.clamp(0.0, 1.0);
        let bx = wx - ex * t;
        let by = wy - ey * t;
        d = d.min(bx * bx + by * by);
        // Winding number contribution (crossing test)
        let c1 = py >= verts[i].1;
        let c2 = py < verts[j].1;
        let c3 = ex * wy > ey * wx;
        if (c1 && c2 && c3) || (!c1 && !c2 && !c3) {
            s = -s;
        }
        j = i;
    }
    s * d.sqrt()
}

/// SDF for a cross / plus shape.
fn sdf_cross(px: f32, py: f32, hx: f32, hy: f32) -> f32 {
    let arm_hw = hx * 0.34;
    let arm_hh = hy * 0.34;
    let vertical = sdf_box(px, py, arm_hw, hy);
    let horizontal = sdf_box(px, py, hx, arm_hh);
    vertical.min(horizontal)
}

/// SDF for a checkmark shape.
fn sdf_check(px: f32, py: f32, hx: f32, hy: f32) -> f32 {
    // Checkmark as two thick line segments
    let thickness = hx.min(hy) * 0.2;

    // Segment 1: from bottom-left area to bottom-center (the short stroke)
    let ax1 = -hx * 0.7;
    let ay1 = hy * 0.0;
    let bx1 = -hx * 0.1;
    let by1 = hy * 0.6;

    // Segment 2: from bottom-center to top-right (the long stroke)
    let ax2 = -hx * 0.1;
    let ay2 = hy * 0.6;
    let bx2 = hx * 0.8;
    let by2 = -hy * 0.7;

    let d1 = sdf_line_segment(px, py, ax1, ay1, bx1, by1) - thickness;
    let d2 = sdf_line_segment(px, py, ax2, ay2, bx2, by2) - thickness;
    d1.min(d2)
}

/// SDF for distance to a line segment.
#[inline]
fn sdf_line_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = bx - ax;
    let dy = by - ay;
    let t = ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy);
    let t = t.clamp(0.0, 1.0);
    let cx = ax + t * dx;
    let cy = ay + t * dy;
    ((px - cx) * (px - cx) + (py - cy) * (py - cy)).sqrt()
}

/// Compute the SDF value for a shape kind at local coordinates (centered at origin).
pub fn shape_sdf(kind: ShapeKind, px: f32, py: f32, hx: f32, hy: f32, corner_radius: f32) -> f32 {
    match kind {
        ShapeKind::Rectangle => sdf_box(px, py, hx, hy),
        ShapeKind::Ellipse => sdf_ellipse(px, py, hx, hy),
        ShapeKind::RoundedRect => sdf_rounded_box(px, py, hx, hy, corner_radius),
        ShapeKind::Triangle => sdf_triangle_box(px, py, hx, hy),
        ShapeKind::RightTriangle => sdf_right_triangle(px, py, hx, hy),
        ShapeKind::Trapezoid => sdf_trapezoid(px, py, hx, hy),
        ShapeKind::Parallelogram => sdf_parallelogram(px, py, hx, hy),
        ShapeKind::Diamond => sdf_diamond(px, py, hx, hy),
        ShapeKind::Pentagon => sdf_polygon(px, py, hx.min(hy), 5),
        ShapeKind::Hexagon => sdf_polygon(px, py, hx.min(hy), 6),
        ShapeKind::Octagon => sdf_polygon(px, py, hx.min(hy), 8),
        ShapeKind::Cross => sdf_cross(px, py, hx, hy),
        ShapeKind::Check => sdf_check(px, py, hx, hy),
        ShapeKind::Star5 => sdf_star(px, py, hx.min(hy), hx.min(hy) * 0.4, 5),
        ShapeKind::Star6 => sdf_star(px, py, hx.min(hy), hx.min(hy) * 0.5, 6),
        ShapeKind::Arrow => sdf_arrow(px, py, hx, hy),
        ShapeKind::Heart => sdf_heart(px, py, hx, hy),
    }
}

#[inline]
fn coverage_from_sdf(distance: f32, anti_alias: bool) -> f32 {
    if anti_alias {
        smoothstep(0.5, -0.5, distance)
    } else if distance < 0.0 {
        1.0
    } else {
        0.0
    }
}

#[inline]
fn rectangle_outline_coverage(
    px: f32,
    py: f32,
    hx: f32,
    hy: f32,
    outline_half: f32,
    anti_alias: bool,
) -> f32 {
    let outer_cov = coverage_from_sdf(
        sdf_box(px, py, hx + outline_half, hy + outline_half),
        anti_alias,
    );
    let inner_hx = (hx - outline_half).max(0.0);
    let inner_hy = (hy - outline_half).max(0.0);
    let inner_cov = if inner_hx > 0.0 && inner_hy > 0.0 {
        coverage_from_sdf(sdf_box(px, py, inner_hx, inner_hy), anti_alias)
    } else {
        0.0
    };
    (outer_cov - inner_cov).clamp(0.0, 1.0)
}

/// Outline for the Cross shape.
/// Each arm of the cross is a box; we expand/shrink each box component
/// individually by `outline_half` so all four sides have uniform thickness.
#[inline]
fn cross_outline_coverage(
    px: f32,
    py: f32,
    hx: f32,
    hy: f32,
    outline_half: f32,
    anti_alias: bool,
) -> f32 {
    let arm_hw = hx * 0.34;
    let arm_hh = hy * 0.34;
    let ol = outline_half;

    // Outer cross: each box expanded by ol in all directions
    let outer_sdf = sdf_box(px, py, arm_hw + ol, hy + ol).min(sdf_box(px, py, hx + ol, arm_hh + ol));
    let outer_cov = coverage_from_sdf(outer_sdf, anti_alias);

    // Inner cross: each box shrunk by ol
    let i_arm_hw = (arm_hw - ol).max(0.0);
    let i_hy    = (hy - ol).max(0.0);
    let i_hx    = (hx - ol).max(0.0);
    let i_arm_hh = (arm_hh - ol).max(0.0);
    let inner_cov = if i_arm_hw > 0.0 || i_hy > 0.0 {
        let inner_sdf = sdf_box(px, py, i_arm_hw, i_hy).min(sdf_box(px, py, i_hx, i_arm_hh));
        coverage_from_sdf(inner_sdf, anti_alias)
    } else {
        0.0
    };
    (outer_cov - inner_cov).clamp(0.0, 1.0)
}

/// Outline for the Trapezoid shape using proper per-edge normal offset (miter joints).
/// Offsets each edge by `outline_half` along its outward normal, then intersects
/// adjacent offset lines to find the mitered corner vertices. This gives uniform
/// outline width along all edges including the slanted sides.
#[inline]
fn trapezoid_outline_coverage(
    px: f32,
    py: f32,
    hx: f32,
    hy: f32,
    outline_half: f32,
    anti_alias: bool,
) -> f32 {
    let top_hw = hx * 0.55;
    let s = hx * 0.45; // horizontal delta of right slanted edge: hx - top_hw
    // Length of the right slanted edge (from top-right to bottom-right vertex)
    let len = (4.0 * hy * hy + s * s).sqrt();
    let ol = outline_half;

    // Corner x-offsets derived from intersecting adjacent offset edges:
    //   top corners:    expand by ol*(len - s)/(2*hy)
    //   bottom corners: expand by ol*(len + s)/(2*hy)
    let expand_top = ol * (len - s) / (2.0 * hy);
    let expand_bot = ol * (len + s) / (2.0 * hy);

    // Outer polygon vertices (CCW: top-left, top-right, bot-right, bot-left)
    let outer_verts = [
        (-(top_hw + expand_top), -(hy + ol)),
        (  top_hw + expand_top,  -(hy + ol)),
        (  hx + expand_bot,       hy + ol),
        (-(hx + expand_bot),      hy + ol),
    ];
    let outer_cov = coverage_from_sdf(sdf_convex_polygon(&outer_verts, px, py), anti_alias);

    // Inner polygon: shrink along the same normals
    let inner_top_hw = (top_hw - expand_top).max(0.0);
    let inner_hx     = (hx - expand_bot).max(0.0);
    let inner_hy     = (hy - ol).max(0.0);
    let inner_cov = if inner_top_hw > 0.0 && inner_hx > 0.0 && inner_hy > 0.0 {
        let inner_verts = [
            (-inner_top_hw, -inner_hy),
            ( inner_top_hw, -inner_hy),
            ( inner_hx,      inner_hy),
            (-inner_hx,      inner_hy),
        ];
        coverage_from_sdf(sdf_convex_polygon(&inner_verts, px, py), anti_alias)
    } else {
        0.0
    };
    (outer_cov - inner_cov).clamp(0.0, 1.0)
}

/// Rasterize a shape into an RGBA buffer.
///
/// Returns `(buf, buf_w, buf_h, offset_x, offset_y)` where offset is the
/// top-left corner of the buffer in canvas coordinates.
pub fn rasterize_shape(
    placed: &PlacedShape,
    canvas_w: u32,
    canvas_h: u32,
) -> (Vec<u8>, u32, u32, i32, i32) {
    // Compute axis-aligned bounding box that contains the rotated shape
    let cos_r = placed.rotation.cos();
    let sin_r = placed.rotation.sin();
    // Corners of the un-rotated box
    let corners = [
        (-placed.hw, -placed.hh),
        (placed.hw, -placed.hh),
        (placed.hw, placed.hh),
        (-placed.hw, placed.hh),
    ];
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for (cx, cy) in &corners {
        let rx = cx * cos_r - cy * sin_r + placed.cx;
        let ry = cx * sin_r + cy * cos_r + placed.cy;
        min_x = min_x.min(rx);
        min_y = min_y.min(ry);
        max_x = max_x.max(rx);
        max_y = max_y.max(ry);
    }
    // Add padding for outline width + AA
    let pad = placed.outline_width + 2.0;
    min_x -= pad;
    min_y -= pad;
    max_x += pad;
    max_y += pad;

    // Clamp to canvas
    let x0 = (min_x.floor() as i32).max(0);
    let y0 = (min_y.floor() as i32).max(0);
    let x1 = (max_x.ceil() as i32).min(canvas_w as i32);
    let y1 = (max_y.ceil() as i32).min(canvas_h as i32);
    let buf_w = (x1 - x0).max(0) as u32;
    let buf_h = (y1 - y0).max(0) as u32;

    if buf_w == 0 || buf_h == 0 {
        return (Vec::new(), 0, 0, 0, 0);
    }

    let row_bytes = buf_w as usize * 4;
    let mut buf = vec![0u8; row_bytes * buf_h as usize];

    let inv_cos = cos_r; // inverse rotation = transpose for rotation matrices
    let inv_sin = -sin_r;

    let primary = placed.primary_color;
    let secondary = placed.secondary_color;
    let outline_half = placed.outline_width * 0.5;
    let aa = placed.anti_alias;
    let fill_mode = placed.fill_mode;
    let hx = placed.hw;
    let hy = placed.hh;
    let kind = placed.kind;
    let corner_radius = placed.corner_radius;
    let cx = placed.cx;
    let cy = placed.cy;

    buf.par_chunks_mut(row_bytes)
        .enumerate()
        .for_each(|(row, row_buf)| {
            let py_canvas = (y0 + row as i32) as f32 + 0.5;
            for col in 0..buf_w as usize {
                let px_canvas = (x0 + col as i32) as f32 + 0.5;

                // Transform to shape-local coordinates (inverse rotate around center)
                let dx = px_canvas - cx;
                let dy = py_canvas - cy;
                let lx = dx * inv_cos - dy * inv_sin;
                let ly = dx * inv_sin + dy * inv_cos;

                let d = shape_sdf(kind, lx, ly, hx, hy, corner_radius);

                let (color, coverage) = match fill_mode {
                    ShapeFillMode::Filled => {
                        let cov = coverage_from_sdf(d, aa);
                        (primary, cov)
                    }
                    ShapeFillMode::Outline => {
                        let cov = if kind == ShapeKind::Rectangle {
                            rectangle_outline_coverage(lx, ly, hx, hy, outline_half, aa)
                        } else if kind == ShapeKind::Cross {
                            cross_outline_coverage(lx, ly, hx, hy, outline_half, aa)
                        } else if kind == ShapeKind::Trapezoid {
                            trapezoid_outline_coverage(lx, ly, hx, hy, outline_half, aa)
                        } else {
                            let band = d.abs() - outline_half;
                            coverage_from_sdf(band, aa)
                        };
                        (primary, cov)
                    }
                    ShapeFillMode::Both => {
                        // Fill interior with secondary, outline with primary
                        let fill_cov = coverage_from_sdf(d, aa);
                        let outline_cov = if kind == ShapeKind::Rectangle {
                            rectangle_outline_coverage(lx, ly, hx, hy, outline_half, aa)
                        } else if kind == ShapeKind::Cross {
                            cross_outline_coverage(lx, ly, hx, hy, outline_half, aa)
                        } else if kind == ShapeKind::Trapezoid {
                            trapezoid_outline_coverage(lx, ly, hx, hy, outline_half, aa)
                        } else {
                            let band = d.abs() - outline_half;
                            coverage_from_sdf(band, aa)
                        };

                        if outline_cov > 0.001 {
                            // Outline on top
                            let oa = outline_cov;
                            let fa = fill_cov * (1.0 - oa);
                            let total_a = oa + fa;
                            if total_a > 0.0 {
                                let r =
                                    (primary[0] as f32 * oa + secondary[0] as f32 * fa) / total_a;
                                let g =
                                    (primary[1] as f32 * oa + secondary[1] as f32 * fa) / total_a;
                                let b =
                                    (primary[2] as f32 * oa + secondary[2] as f32 * fa) / total_a;
                                let a =
                                    (primary[3] as f32 * oa + secondary[3] as f32 * fa) / total_a;
                                ([r as u8, g as u8, b as u8, a as u8], total_a)
                            } else {
                                ([0, 0, 0, 0], 0.0)
                            }
                        } else {
                            (secondary, fill_cov)
                        }
                    }
                };

                if coverage > 0.001 {
                    let idx = col * 4;
                    let a = (color[3] as f32 * coverage).round().min(255.0) as u8;
                    row_buf[idx] = color[0];
                    row_buf[idx + 1] = color[1];
                    row_buf[idx + 2] = color[2];
                    row_buf[idx + 3] = a;
                }
            }
        });

    (buf, buf_w, buf_h, x0, y0)
}

/// Rasterize a shape into a caller-provided RGBA buffer (reused across frames).
///
/// Returns `(buf_w, buf_h, offset_x, offset_y)`. The buffer is resized and zeroed
/// to fit the shape's bounding box; capacity is preserved to avoid reallocation.
pub fn rasterize_shape_into(
    placed: &PlacedShape,
    canvas_w: u32,
    canvas_h: u32,
    buf: &mut Vec<u8>,
) -> (u32, u32, i32, i32) {
    let cos_r = placed.rotation.cos();
    let sin_r = placed.rotation.sin();
    let corners = [
        (-placed.hw, -placed.hh),
        (placed.hw, -placed.hh),
        (placed.hw, placed.hh),
        (-placed.hw, placed.hh),
    ];
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for (cx, cy) in &corners {
        let rx = cx * cos_r - cy * sin_r + placed.cx;
        let ry = cx * sin_r + cy * cos_r + placed.cy;
        min_x = min_x.min(rx);
        min_y = min_y.min(ry);
        max_x = max_x.max(rx);
        max_y = max_y.max(ry);
    }
    let pad = placed.outline_width + 2.0;
    min_x -= pad;
    min_y -= pad;
    max_x += pad;
    max_y += pad;

    let x0 = (min_x.floor() as i32).max(0);
    let y0 = (min_y.floor() as i32).max(0);
    let x1 = (max_x.ceil() as i32).min(canvas_w as i32);
    let y1 = (max_y.ceil() as i32).min(canvas_h as i32);
    let buf_w = (x1 - x0).max(0) as u32;
    let buf_h = (y1 - y0).max(0) as u32;

    if buf_w == 0 || buf_h == 0 {
        return (0, 0, 0, 0);
    }

    let row_bytes = buf_w as usize * 4;
    let total = row_bytes * buf_h as usize;
    buf.resize(total, 0);
    // Zero the buffer (resize only zeroes newly-added bytes)
    buf.iter_mut().for_each(|b| *b = 0);

    let inv_cos = cos_r;
    let inv_sin = -sin_r;
    let primary = placed.primary_color;
    let secondary = placed.secondary_color;
    let outline_half = placed.outline_width * 0.5;
    let aa = placed.anti_alias;
    let fill_mode = placed.fill_mode;
    let hx = placed.hw;
    let hy = placed.hh;
    let kind = placed.kind;
    let corner_radius = placed.corner_radius;
    let cx = placed.cx;
    let cy = placed.cy;

    buf.par_chunks_mut(row_bytes)
        .enumerate()
        .for_each(|(row, row_buf)| {
            let py_canvas = (y0 + row as i32) as f32 + 0.5;
            for col in 0..buf_w as usize {
                let px_canvas = (x0 + col as i32) as f32 + 0.5;
                let dx = px_canvas - cx;
                let dy = py_canvas - cy;
                let lx = dx * inv_cos - dy * inv_sin;
                let ly = dx * inv_sin + dy * inv_cos;
                let d = shape_sdf(kind, lx, ly, hx, hy, corner_radius);

                let (color, coverage) = match fill_mode {
                    ShapeFillMode::Filled => {
                        let cov = coverage_from_sdf(d, aa);
                        (primary, cov)
                    }
                    ShapeFillMode::Outline => {
                        let cov = if kind == ShapeKind::Rectangle {
                            rectangle_outline_coverage(lx, ly, hx, hy, outline_half, aa)
                        } else if kind == ShapeKind::Cross {
                            cross_outline_coverage(lx, ly, hx, hy, outline_half, aa)
                        } else if kind == ShapeKind::Trapezoid {
                            trapezoid_outline_coverage(lx, ly, hx, hy, outline_half, aa)
                        } else {
                            let band = d.abs() - outline_half;
                            coverage_from_sdf(band, aa)
                        };
                        (primary, cov)
                    }
                    ShapeFillMode::Both => {
                        let fill_cov = coverage_from_sdf(d, aa);
                        let outline_cov = if kind == ShapeKind::Rectangle {
                            rectangle_outline_coverage(lx, ly, hx, hy, outline_half, aa)
                        } else if kind == ShapeKind::Cross {
                            cross_outline_coverage(lx, ly, hx, hy, outline_half, aa)
                        } else if kind == ShapeKind::Trapezoid {
                            trapezoid_outline_coverage(lx, ly, hx, hy, outline_half, aa)
                        } else {
                            let band = d.abs() - outline_half;
                            coverage_from_sdf(band, aa)
                        };
                        if outline_cov > 0.001 {
                            let oa = outline_cov;
                            let fa = fill_cov * (1.0 - oa);
                            let total_a = oa + fa;
                            if total_a > 0.0 {
                                let r =
                                    (primary[0] as f32 * oa + secondary[0] as f32 * fa) / total_a;
                                let g =
                                    (primary[1] as f32 * oa + secondary[1] as f32 * fa) / total_a;
                                let b =
                                    (primary[2] as f32 * oa + secondary[2] as f32 * fa) / total_a;
                                let a =
                                    (primary[3] as f32 * oa + secondary[3] as f32 * fa) / total_a;
                                ([r as u8, g as u8, b as u8, a as u8], total_a)
                            } else {
                                ([0, 0, 0, 0], 0.0)
                            }
                        } else {
                            (secondary, fill_cov)
                        }
                    }
                };

                if coverage > 0.001 {
                    let idx = col * 4;
                    let a = (color[3] as f32 * coverage).round().min(255.0) as u8;
                    row_buf[idx] = color[0];
                    row_buf[idx + 1] = color[1];
                    row_buf[idx + 2] = color[2];
                    row_buf[idx + 3] = a;
                }
            }
        });

    (buf_w, buf_h, x0, y0)
}

/// Smoothstep between edge0 and edge1.
#[inline]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::{ShapeKind, shape_sdf};

    #[test]
    fn trapezoid_signs_are_stable() {
        assert!(shape_sdf(ShapeKind::Trapezoid, 0.0, 0.0, 40.0, 30.0, 0.0) < 0.0);
        assert!(shape_sdf(ShapeKind::Trapezoid, 35.0, -25.0, 40.0, 30.0, 0.0) > 0.0);
        assert!(shape_sdf(ShapeKind::Trapezoid, 30.0, 20.0, 40.0, 30.0, 0.0) < 0.0);
    }

    #[test]
    fn cross_signs_are_stable() {
        assert!(shape_sdf(ShapeKind::Cross, 0.0, 0.0, 40.0, 40.0, 0.0) < 0.0);
        assert!(shape_sdf(ShapeKind::Cross, 34.0, 0.0, 40.0, 40.0, 0.0) < 0.0);
        assert!(shape_sdf(ShapeKind::Cross, 30.0, 30.0, 40.0, 40.0, 0.0) > 0.0);
    }

    #[test]
    fn heart_signs_are_stable() {
        assert!(shape_sdf(ShapeKind::Heart, 0.0, 10.0, 40.0, 40.0, 0.0) < 0.0);
        assert!(shape_sdf(ShapeKind::Heart, 0.0, -36.0, 40.0, 40.0, 0.0) > 0.0);
        assert!(shape_sdf(ShapeKind::Heart, 0.0, 41.0, 40.0, 40.0, 0.0) > 0.0);
    }
}
