//! Annotative scaling: text, dimensions, leaders, multileaders and hatches marked annotative keep their size on
//! paper. They are drawn in model space at the current annotation scale (CANNOSCALE): text 2.5 high on paper is
//! 125 high at 1:50. Changing the annotation scale resizes every annotative object in model space to match.

use super::editing::rehatched;
use super::entities::{Entity, Shape};

/// An annotation scale: so many paper units to so many drawing units, as 1:50 is one to fifty.
#[derive(Clone, Debug, PartialEq)]
pub struct AnnotationScale {
    pub name: String,
    pub paper: f64,
    pub drawing: f64,
}

impl AnnotationScale {
    pub fn new(paper: f64, drawing: f64) -> AnnotationScale {
        let short = |v: f64| if v.fract() == 0.0 { format!("{v:.0}") } else { format!("{v}") };
        AnnotationScale { name: format!("{}:{}", short(paper), short(drawing)), paper, drawing }
    }

    /// How many times larger than on paper an annotative object is drawn.
    pub fn factor(&self) -> f64 {
        self.drawing / self.paper
    }
}

impl Default for AnnotationScale {
    fn default() -> AnnotationScale {
        AnnotationScale::new(1.0, 1.0)
    }
}

/// AutoCAD's default scale list for metric drawings.
pub fn standard_scales() -> Vec<AnnotationScale> {
    let mut scales: Vec<AnnotationScale> =
        [1.0, 2.0, 4.0, 5.0, 8.0, 10.0, 16.0, 20.0, 30.0, 40.0, 50.0, 100.0].iter().map(|d| AnnotationScale::new(1.0, *d)).collect();
    scales.extend([2.0, 4.0, 8.0, 10.0, 100.0].iter().map(|p| AnnotationScale::new(*p, 1.0)));
    scales
}

/// A scale as typed or as a file names it: `1:50`, `1/50` or `2:1`, or a name from the list.
pub fn parse_scale(text: &str, list: &[AnnotationScale]) -> Option<AnnotationScale> {
    let text = text.trim();
    if let Some(found) = list.iter().find(|s| s.name.eq_ignore_ascii_case(text)) {
        return Some(found.clone());
    }
    let (paper, drawing) = text.split_once(':').or_else(|| text.split_once('/'))?;
    let (paper, drawing): (f64, f64) = (paper.trim().parse().ok()?, drawing.trim().parse().ok()?);
    (paper > 0.0 && drawing > 0.0 && paper.is_finite() && drawing.is_finite()).then(|| AnnotationScale::new(paper, drawing))
}

/// Whether an object of this kind can be annotative.
pub fn can_be_annotative(shape: &Shape) -> bool {
    match shape {
        Shape::Text { .. } | Shape::Dimension { .. } | Shape::Annotation { .. } | Shape::Multileader { .. } => true,
        Shape::Hatch { solid, gradient, pattern, .. } => !solid && gradient.is_none() && !pattern.is_empty(),
        _ => false,
    }
}

/// An annotative object's sizes multiplied by `ratio`, where it sits staying where it is: text grows from its
/// insertion point, a dimension's text and arrows grow, a multileader's text and arrows grow from its landing, and
/// a hatch's pattern spreads out. Anything else comes back as it is.
pub fn resized(entity: &Entity, ratio: f64) -> Entity {
    if !entity.annotative || !(ratio.is_finite() && ratio > 0.0) || ratio == 1.0 {
        return entity.clone();
    }
    let mut shape = entity.shape.clone();
    match &mut shape {
        Shape::Text { height, .. } => *height *= ratio,
        Shape::Dimension { look, .. } | Shape::Annotation { look, .. } => look.scale *= ratio,
        Shape::Multileader { landing, text_position, text_height, arrow_size, dogleg, .. } => {
            *text_height *= ratio;
            *arrow_size *= ratio;
            *dogleg *= ratio;
            *text_position = *landing + (*text_position - *landing) * ratio;
        }
        Shape::Hatch { name, scale, angle, .. } => {
            let (name, scale, angle) = (name.clone(), *scale * ratio, *angle);
            if let Some(again) = rehatched(&entity.shape, &name, scale, angle) {
                shape = again;
            }
        }
        _ => {}
    }
    entity.with_shape(shape)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::{line, text};
    use crate::model::geometry::pt;

    #[test]
    fn scales_are_named_and_parsed() {
        let list = standard_scales();
        assert_eq!(list[10].name, "1:50");
        assert_eq!(list.iter().find(|s| s.name == "2:1").map(AnnotationScale::factor), Some(0.5));
        assert_eq!(parse_scale("1/25", &list).map(|s| (s.factor(), s.name)), Some((25.0, "1:25".into())));
        assert_eq!(parse_scale(" 1:100 ", &list).map(|s| s.factor()), Some(100.0));
        assert!(parse_scale("1:0", &list).is_none() && parse_scale("big", &list).is_none());
    }

    #[test]
    fn only_annotative_objects_are_resized() {
        let mut label = text(pt(1.0, 1.0), "A", 2.5, 0.0);
        assert_eq!(resized(&label, 50.0).shape, label.shape, "not annotative");
        label.annotative = true;
        let Shape::Text { height, position, .. } = resized(&label, 50.0).shape else { panic!("text") };
        assert_eq!((height, position), (125.0, pt(1.0, 1.0)));
        let mut edge = line(pt(0.0, 0.0), pt(1.0, 0.0));
        edge.annotative = true;
        assert_eq!(resized(&edge, 50.0).shape, edge.shape, "a line has no paper size");
        assert!(!can_be_annotative(&edge.shape) && can_be_annotative(&label.shape));
    }
}
