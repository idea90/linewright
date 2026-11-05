//! What SPLINEDIT does to a spline's definition: knots made explicit, a control point added without changing the
//! curve, closing, opening, reversing, and finding a control point.

use super::geometry::Point;
use super::parts::clamped_knots;

/// A spline's definition, its knots and weights filled in.
#[derive(Clone, Debug, PartialEq)]
pub struct SplineData {
    pub degree: usize,
    pub control: Vec<Point>,
    pub knots: Vec<f64>,
    pub weights: Vec<f64>,
}

impl SplineData {
    pub fn new(degree: usize, control: &[Point], knots: &[f64], weights: &[f64]) -> SplineData {
        let degree = degree.clamp(1, control.len().saturating_sub(1).max(1));
        let knots = if knots.len() == control.len() + degree + 1 { knots.to_vec() } else { clamped_knots(degree, control.len()) };
        let weights = if weights.len() == control.len() { weights.to_vec() } else { vec![1.0; control.len()] };
        SplineData { degree, control: control.to_vec(), knots, weights }
    }

    /// Weights only when some differ from 1, as a plain spline is written.
    pub fn stored_weights(&self) -> Vec<f64> {
        if self.weights.iter().all(|w| *w == 1.0) { vec![] } else { self.weights.clone() }
    }

    fn point_at(&self, u: f64) -> Point {
        // De Boor on the homogeneous points.
        let p = self.degree;
        let last = self.control.len() - 1;
        let span = (p..=last).rev().find(|&i| self.knots[i] <= u).unwrap_or(p);
        let mut d: Vec<(f64, f64, f64)> = (0..=p)
            .map(|j| {
                let i = span - p + j;
                let w = self.weights[i];
                (self.control[i].x * w, self.control[i].y * w, w)
            })
            .collect();
        for r in 1..=p {
            for j in (r..=p).rev() {
                let i = span - p + j;
                let denominator = self.knots[i + p - r + 1] - self.knots[i];
                let a = if denominator > 0.0 { (u - self.knots[i]) / denominator } else { 0.0 };
                let (low, high) = (d[j - 1], d[j]);
                d[j] = (low.0 + (high.0 - low.0) * a, low.1 + (high.1 - low.1) * a, low.2 + (high.2 - low.2) * a);
            }
        }
        let (x, y, w) = d[p];
        super::geometry::pt(x / w, y / w)
    }

    /// The parameter on the curve nearest a point.
    pub fn nearest_parameter(&self, to: Point) -> f64 {
        const SAMPLES: usize = 400;
        let (low, high) = (self.knots[self.degree], self.knots[self.control.len()]);
        let at = |i: usize| low + (high - low) * i as f64 / SAMPLES as f64;
        (0..=SAMPLES).map(at).min_by(|a, b| self.point_at(*a).dist(to).total_cmp(&self.point_at(*b).dist(to))).unwrap_or(low)
    }

    /// One more control point with the curve unchanged (Boehm's knot insertion).
    pub fn insert_knot(&mut self, u: f64) {
        let p = self.degree;
        let last = self.control.len() - 1;
        let span = (p..=last).rev().find(|&i| self.knots[i] <= u).unwrap_or(p);
        let homogeneous: Vec<(f64, f64, f64)> = self.control.iter().zip(&self.weights).map(|(c, w)| (c.x * w, c.y * w, *w)).collect();
        let mut next = Vec::with_capacity(homogeneous.len() + 1);
        for i in 0..=homogeneous.len() {
            let point = if i + p <= span {
                homogeneous[i]
            } else if i > span {
                homogeneous[i - 1]
            } else {
                let a = (u - self.knots[i]) / (self.knots[i + p] - self.knots[i]);
                let (low, high) = (homogeneous[i - 1], homogeneous[i]);
                (low.0 + (high.0 - low.0) * a, low.1 + (high.1 - low.1) * a, low.2 + (high.2 - low.2) * a)
            };
            next.push(point);
        }
        self.control = next.iter().map(|(x, y, w)| super::geometry::pt(x / w, y / w)).collect();
        self.weights = next.iter().map(|h| h.2).collect();
        self.knots.insert(span + 1, u);
    }

    /// The same curve the other way along.
    pub fn reverse(&mut self) {
        self.control.reverse();
        self.weights.reverse();
        let top = *self.knots.last().unwrap_or(&0.0);
        self.knots = self.knots.iter().rev().map(|k| top - k).collect();
    }

    /// Smoothly closed: the first control points repeat at the end, on evenly spaced knots.
    pub fn close(&mut self) {
        for i in 0..self.degree {
            self.control.push(self.control[i]);
            self.weights.push(self.weights[i]);
        }
        self.knots = (0..self.control.len() + self.degree + 1).map(|i| i as f64).collect();
    }

    /// Opened again: the repeated control points go, and the curve runs end to end of those left.
    pub fn open(&mut self) {
        let keep = self.control.len().saturating_sub(self.degree).max(2);
        self.control.truncate(keep);
        self.weights.truncate(keep);
        self.degree = self.degree.min(keep - 1);
        self.knots = clamped_knots(self.degree, keep);
    }

    /// Without a control point, the knots spaced afresh. False when too few would be left.
    pub fn remove_control(&mut self, index: usize) -> bool {
        if self.control.len() <= self.degree + 1 || index >= self.control.len() {
            return false;
        }
        self.control.remove(index);
        self.weights.remove(index);
        self.knots = clamped_knots(self.degree, self.control.len());
        true
    }

    /// The control point nearest a point.
    pub fn nearest_control(&self, to: Point) -> usize {
        (0..self.control.len()).min_by(|a, b| self.control[*a].dist(to).total_cmp(&self.control[*b].dist(to))).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::geometry::pt;
    use crate::model::parts::spline_points;

    fn sample() -> SplineData {
        SplineData::new(3, &[pt(0.0, 0.0), pt(2.0, 5.0), pt(6.0, 5.0), pt(8.0, 0.0), pt(12.0, 3.0)], &[], &[])
    }

    #[test]
    fn adding_a_control_point_leaves_the_curve_where_it_was() {
        let mut spline = sample();
        let before: Vec<Point> = (0..=20).map(|i| spline.point_at(i as f64 / 20.0)).collect();
        spline.insert_knot(0.3);
        assert_eq!(spline.control.len(), 6);
        assert_eq!(spline.knots.len(), 6 + 3 + 1);
        for (i, old) in before.iter().enumerate() {
            assert!(spline.point_at(i as f64 / 20.0).dist(*old) < 1e-9);
        }
    }

    #[test]
    fn reversing_runs_the_same_curve_backward() {
        let mut spline = sample();
        let forward = spline.point_at(0.25);
        spline.reverse();
        assert!(spline.point_at(0.75).dist(forward) < 1e-9);
    }

    #[test]
    fn closing_joins_the_ends_and_opening_undoes_it() {
        let mut spline = sample();
        spline.close();
        assert_eq!(spline.control.len(), 8);
        let points = spline_points(spline.degree, &spline.control, &spline.knots, &[]);
        assert!(points[0].dist(*points.last().unwrap()) < 1e-6, "{:?} {:?}", points[0], points.last());
        spline.open();
        assert_eq!(spline.control.len(), 5);
        assert_eq!(spline.control[0], pt(0.0, 0.0));
    }
}
