//! A one-page vector PDF: stroked paths, filled triangles and Helvetica text, in points (1/72 inch), y up.

use std::fmt::Write;

use crate::fonts::PathOp;
use crate::model::geometry::Point;

pub struct PdfPage {
    width: f64,
    height: f64,
    content: String,
    /// Pictures placed on the page: width and height in pixels, and their RGB, compressed.
    images: Vec<(u32, u32, Vec<u8>)>,
}

/// Helvetica advance widths, in thousandths of the font size, for the characters a dimension shows.
pub fn helvetica_width(text: &str, size: f64) -> f64 {
    text.chars()
        .map(|c| match c {
            '0'..='9' => 556.0,
            '.' | ',' | ' ' | ':' | ';' => 278.0,
            '-' => 333.0,
            'i' | 'j' | 'l' | 'I' => 222.0,
            'm' | 'M' | 'W' => 833.0,
            'A'..='Z' => 667.0,
            _ => 556.0,
        })
        .sum::<f64>()
        * size
        / 1000.0
}

fn num(value: f64) -> String {
    let text = format!("{value:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0".into() } else { text.into() }
}

/// Text for a PDF string in WinAnsi: brackets and backslashes escaped, anything outside Latin-1 as '?'.
fn pdf_string(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) >= 32 && (c as u32) < 127 => out.push(c),
            c if (c as u32) >= 160 && (c as u32) < 256 => {
                let _ = write!(out, "\\{:03o}", c as u32);
            }
            _ => out.push('?'),
        }
    }
    out
}

impl PdfPage {
    pub fn new(width: f64, height: f64) -> PdfPage {
        PdfPage { width, height, content: String::from("1 J 1 j\n"), images: vec![] }
    }

    /// Remember the clipping, to go back to it with `restore`.
    pub fn save(&mut self) {
        self.content.push_str("q\n");
    }

    pub fn restore(&mut self) {
        self.content.push_str("Q\n");
    }

    /// Everything drawn after this is cut off outside the rectangle.
    pub fn clip(&mut self, x: f64, y: f64, width: f64, height: f64) {
        let _ = writeln!(self.content, "{} {} {} {} re W n", num(x), num(y), num(width), num(height));
    }

    pub fn stroke(&mut self, points: &[Point], rgb: (u8, u8, u8), width: f64, dashes: &[f64]) {
        if points.len() < 2 {
            return;
        }
        let pattern: Vec<String> = dashes.iter().map(|d| num(*d)).collect();
        let _ = writeln!(self.content, "{} RG {} w [{}] 0 d", color(rgb), num(width), pattern.join(" "));
        let _ = write!(self.content, "{} {} m", num(points[0].x), num(points[0].y));
        for p in &points[1..] {
            let _ = write!(self.content, " {} {} l", num(p.x), num(p.y));
        }
        self.content.push_str(" S\n");
    }

    pub fn fill(&mut self, corners: &[Point], rgb: (u8, u8, u8)) {
        if corners.len() < 3 {
            return;
        }
        let _ = write!(self.content, "{} rg {} {} m", color(rgb), num(corners[0].x), num(corners[0].y));
        for p in &corners[1..] {
            let _ = write!(self.content, " {} {} l", num(p.x), num(p.y));
        }
        self.content.push_str(" h f\n");
    }

    /// Fill a shape made of lines and curves, such as the outlines of text. Holes are left where outlines wind
    /// the other way, as fonts draw them.
    pub fn fill_path(&mut self, ops: &[PathOp], rgb: (u8, u8, u8)) {
        if ops.is_empty() {
            return;
        }
        let _ = write!(self.content, "{} rg", color(rgb));
        for op in ops {
            let _ = match op {
                PathOp::Move(p) => write!(self.content, " {} {} m", num(p.x), num(p.y)),
                PathOp::Line(p) => write!(self.content, " {} {} l", num(p.x), num(p.y)),
                PathOp::Curve(a, b, p) => {
                    write!(self.content, " {} {} {} {} {} {} c", num(a.x), num(a.y), num(b.x), num(b.y), num(p.x), num(p.y))
                }
                PathOp::Close => write!(self.content, " h"),
            };
        }
        self.content.push_str(" f\n");
    }

    /// Fill an area of loops, leaving holes where loops lie inside loops (the even-odd rule).
    pub fn fill_area(&mut self, loops: &[Vec<Point>], rgb: (u8, u8, u8)) {
        let _ = write!(self.content, "{} rg", color(rgb));
        for points in loops.iter().filter(|l| l.len() >= 3) {
            let _ = write!(self.content, " {} {} m", num(points[0].x), num(points[0].y));
            for p in &points[1..] {
                let _ = write!(self.content, " {} {} l", num(p.x), num(p.y));
            }
            self.content.push_str(" h");
        }
        self.content.push_str(" f*\n");
    }

    /// Text with its baseline starting at the origin, turned counter-clockwise by the angle in degrees.
    pub fn text(&mut self, origin: Point, rotation: f64, size: f64, value: &str, rgb: (u8, u8, u8)) {
        let (sin, cos) = rotation.to_radians().sin_cos();
        let _ = writeln!(
            self.content,
            "BT /F1 {} Tf {} rg {} {} {} {} {} {} Tm ({}) Tj ET",
            num(size),
            color(rgb),
            num(cos),
            num(sin),
            num(-sin),
            num(cos),
            num(origin.x),
            num(origin.y),
            pdf_string(value)
        );
    }

    /// A picture stretched over a parallelogram: its lower left corner, then the vectors along its bottom and up
    /// its side, in page points. `rgb` holds its pixels row by row from the top.
    pub fn image(&mut self, corner: Point, along: Point, up: Point, (width, height): (u32, u32), rgb: &[u8]) {
        use std::io::Write as _;
        let mut packed = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        if packed.write_all(rgb).is_err() {
            return;
        }
        let Ok(packed) = packed.finish() else { return };
        let name = self.images.len();
        self.images.push((width, height, packed));
        let _ = writeln!(
            self.content,
            "q {} {} {} {} {} {} cm /Im{name} Do Q",
            num(along.x),
            num(along.y),
            num(up.x),
            num(up.y),
            num(corner.x),
            num(corner.y)
        );
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        // Pictures follow the five fixed objects, from object 6 on.
        let pictures: String = (0..self.images.len()).map(|i| format!("/Im{i} {} 0 R ", 6 + i)).collect();
        let mut objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> /XObject << {pictures}>> >> >>",
                num(self.width),
                num(self.height)
            )
            .into_bytes(),
            format!("<< /Length {} >>\nstream\n{}endstream", self.content.len(), self.content).into_bytes(),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
        ];
        for (width, height, packed) in &self.images {
            let mut object = format!(
                "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
                packed.len()
            )
            .into_bytes();
            object.extend(packed);
            object.extend(b"\nendstream");
            objects.push(object);
        }
        let mut out = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
        let mut offsets = vec![];
        for (index, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n", index + 1).bytes());
            out.extend(body);
            out.extend(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        for offset in offsets {
            out.extend(format!("{offset:010} 00000 n \n").bytes());
        }
        out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).bytes());
        out
    }
}

fn color((r, g, b): (u8, u8, u8)) -> String {
    format!("{} {} {}", num(r as f64 / 255.0), num(g as f64 / 255.0), num(b as f64 / 255.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::geometry::pt;

    #[test]
    fn a_page_is_a_well_formed_pdf() {
        let mut page = PdfPage::new(595.0, 842.0);
        page.stroke(&[pt(0.0, 0.0), pt(100.0, 100.0)], (0, 0, 0), 0.7, &[2.0, 1.0]);
        page.text(pt(10.0, 10.0), 90.0, 12.0, "Size (mm) é", (0, 0, 0));
        let bytes = page.to_bytes();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(text.contains("0 0 m 100 100 l S"));
        assert!(text.contains("(Size \\(mm\\) \\351) Tj"));
        let startxref: usize = text.split("startxref\n").nth(1).unwrap().lines().next().unwrap().parse().unwrap();
        assert!(bytes[startxref..].starts_with(b"xref"), "the cross reference table is where the trailer says");
    }

    #[test]
    fn dimension_digits_are_measured() {
        assert!((helvetica_width("10.00", 10.0) - 25.02).abs() < 1e-9);
    }
}
