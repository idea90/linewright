//! What an entity draws as: the paths, areas, points and texts that `parts` works out and every painter
//! (screen, PDF, picture) draws.

use std::sync::Arc;

use super::entities::{Gradient, HAlign, LINE_PITCH, TextAlign, VAlign};
use super::geometry::{Point, direction};
use super::mtext::RichText;

#[derive(Clone, Debug, PartialEq)]
pub struct TextSpec {
    /// The point the text is aligned on, as `align` says.
    pub origin: Point,
    pub rotation: f64, // degrees, counter-clockwise
    pub height: f64,
    pub text: String,
    pub align: TextAlign,
    pub line_spacing: f64,
    /// The font the drawing asks for, empty for the interface font.
    pub font: String,
    pub look: TextLook,
    /// Runs with looks of their own, in place of `text` when there are any.
    pub rich: Option<Arc<RichText>>,
}

/// How letters are shaped: width, lean, bold and italic.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextLook {
    pub width_factor: f64,
    pub oblique: f64,
    pub bold: bool,
    pub italic: bool,
}

impl Default for TextLook {
    fn default() -> Self {
        TextLook { width_factor: 1.0, oblique: 0.0, bold: false, italic: false }
    }
}

impl TextLook {
    /// How far a letter's top leans over per its height. Italic leans 15 degrees more.
    pub fn shear(&self) -> f64 {
        (self.oblique + if self.italic { 15.0 } else { 0.0 }).to_radians().tan()
    }

    pub fn is_plain(&self) -> bool {
        *self == TextLook::default()
    }
}

/// Fake bold: how far the letters are drawn again to the side, per the text's height.
pub const BOLD_OFFSET: f64 = 0.035;

impl TextSpec {
    /// Where each line's baseline starts, given how wide each line is: lines step down from the first, and each
    /// lines up on the origin as the alignment says.
    pub fn line_origins(&self, widths: &[f64]) -> Vec<Point> {
        let lines = widths.len().max(1) as f64;
        let pitch = self.height * LINE_PITCH * self.line_spacing;
        let first = match self.align.v {
            VAlign::Baseline => 0.0,
            VAlign::Bottom => (lines - 1.0) * pitch,
            VAlign::Middle => (self.height + (lines - 1.0) * pitch) / 2.0 - self.height,
            VAlign::Top => -self.height,
        };
        let (along, up) = (direction(self.rotation), direction(self.rotation + 90.0));
        widths
            .iter()
            .enumerate()
            .map(|(i, width)| {
                let back = match self.align.h {
                    HAlign::Left => 0.0,
                    HAlign::Center => width / 2.0,
                    HAlign::Right => *width,
                };
                self.origin + up * (first - i as f64 * pitch) - along * back
            })
            .collect()
    }

    /// The corners of a box around the text, guessing each character `char_width` heights wide. Generous below
    /// the baseline for descenders.
    pub fn bounding_box(&self, char_width: f64) -> [Point; 4] {
        let lines: Vec<&str> = self.text.split('\n').collect();
        let widths: Vec<f64> = lines.iter().map(|l| l.chars().count().max(1) as f64 * char_width * self.height).collect();
        let origins = self.line_origins(&widths);
        let (along, up) = (direction(self.rotation), direction(self.rotation + 90.0));
        let descent = 0.3 * self.height;
        let local = |p: Point| ((p - self.origin).dot(along), (p - self.origin).dot(up));
        let (mut u0, mut v0, mut u1, mut v1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (origin, width) in origins.iter().zip(&widths) {
            let (u, v) = local(*origin);
            u0 = u0.min(u);
            u1 = u1.max(u + width);
            v0 = v0.min(v - descent);
            v1 = v1.max(v + self.height);
        }
        [(u0, v0), (u1, v0), (u1, v1), (u0, v1)].map(|(u, v)| self.origin + along * u + up * v)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Parts {
    pub paths: Vec<Vec<Point>>,
    /// Filled triangles, such as arrowheads.
    pub fills: Vec<[Point; 3]>,
    /// Filled areas, each a set of loops where loops inside loops are holes.
    pub areas: Vec<Vec<Vec<Point>>>,
    pub points: Vec<Point>,
    pub texts: Vec<TextSpec>,
    /// Gradient fills: the loops, as for areas, and the colors.
    pub gradients: Vec<(Vec<Vec<Point>>, Gradient)>,
    /// Areas painted in the background color, each framed on screen but not on paper: wipeouts.
    pub masks: Vec<Vec<Point>>,
}
