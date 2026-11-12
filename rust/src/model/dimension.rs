use super::geometry::{Point, direction, modulo, pt};

// Drawing units. These match the metric defaults of the DXF dimension style Linewright writes.
pub const TEXT_HEIGHT: f64 = 2.5;
pub const ARROW_LENGTH: f64 = 2.5;
pub const EXTENSION_GAP: f64 = 0.625; // the extension line stops short of the measured point
pub const EXTENSION_OVERSHOOT: f64 = 1.25; // and runs past the dimension line
pub const TEXT_GAP: f64 = 0.625;
pub const LENGTH_DECIMALS: u8 = 2;
/// The dimension style every drawing has.
pub const STANDARD_STYLE: &str = "Standard";
const ARROW_WIDTH_RATIO: f64 = 1.0 / 6.0; // half an arrowhead's width, per length of it
const DOT_RADIUS_RATIO: f64 = 0.125; // a small dot's radius, per arrowhead size
const DOT_SIDES: usize = 12;

/// Text a dimension shows.
#[derive(Clone, Debug, PartialEq)]
pub struct AnnotationText {
    pub text: String,
    pub height: f64,
    pub anchor: Point, // middle of the baseline
    pub middle: Point,
    pub angle: f64, // degrees, readable
}

/// What marks the ends of the dimension line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Arrowhead {
    /// A filled arrowhead, AutoCAD's default.
    #[default]
    Filled,
    /// An oblique stroke, as architectural drawings use.
    Tick,
    /// A small filled dot.
    Dot,
}

/// How a dimension shows its tolerance, as DIMTOL and DIMLIM say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tolerance {
    #[default]
    None,
    /// 25.00±0.10, when the plus and minus are the same.
    Symmetric,
    /// 25.00 with +0.10 over -0.05 beside it.
    Deviation,
    /// The largest and smallest sizes, one over the other.
    Limits,
}

impl Tolerance {
    pub const ALL: [Tolerance; 4] = [Tolerance::None, Tolerance::Symmetric, Tolerance::Deviation, Tolerance::Limits];

    pub fn name(self) -> &'static str {
        match self {
            Tolerance::None => "None",
            Tolerance::Symmetric => "Symmetrical",
            Tolerance::Deviation => "Deviation",
            Tolerance::Limits => "Limits",
        }
    }
}

pub type Segment = (Point, Point);

/// How a dimension looks: the sizes its dimension style gives it, in drawing units before `scale`, and how its
/// measurement is written. Dimensions read from a file keep their own; new ones get Linewright's defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct DimensionLook {
    /// Sizes text, arrows and gaps, like DIMSCALE.
    pub scale: f64,
    pub text_height: f64,
    pub arrow_size: f64,
    pub extension_gap: f64,
    pub extension_overshoot: f64,
    pub text_gap: f64,
    pub decimals: u8,
    /// Decimal places of an angle, like DIMADEC.
    pub angle_decimals: u8,
    /// The measurement shown is the distance times this, like DIMLFAC.
    pub length_factor: f64,
    pub arrowhead: Arrowhead,
    /// Trailing zeros are left off the measurement ("11500" rather than "11500.000"), as DIMZIN can ask.
    pub trim_zeros: bool,
    /// Text that replaces the measurement; "<>" in it stands for the measurement. Empty shows the measurement.
    pub text: String,
    /// Where the middle of the text is, when a file says so: AutoCAD moves text that does not fit between the
    /// extension lines out beside them. None centers it over the dimension line.
    pub text_position: Option<Point>,
    /// The name of the dimension style it follows.
    pub style: String,
    /// The tolerance shown, with how much over (DIMTP) and under (DIMTM) the size may be.
    pub tolerance: Tolerance,
    pub tolerance_plus: f64,
    pub tolerance_minus: f64,
    /// The stacked tolerance text's height per the text height, like DIMTFAC.
    pub tolerance_height: f64,
    /// The text's angle in degrees when DIMTEDIT turned it; None keeps it along the dimension line.
    pub text_rotation: Option<f64>,
}

impl Default for DimensionLook {
    fn default() -> Self {
        DimensionLook::scaled(1.0)
    }
}

impl DimensionLook {
    /// Linewright's own look at a scale.
    pub fn scaled(scale: f64) -> DimensionLook {
        DimensionLook {
            scale,
            text_height: TEXT_HEIGHT,
            arrow_size: ARROW_LENGTH,
            extension_gap: EXTENSION_GAP,
            extension_overshoot: EXTENSION_OVERSHOOT,
            text_gap: TEXT_GAP,
            decimals: LENGTH_DECIMALS,
            angle_decimals: 0,
            length_factor: 1.0,
            arrowhead: Arrowhead::Filled,
            trim_zeros: false,
            text: String::new(),
            text_position: None,
            style: STANDARD_STYLE.to_string(),
            tolerance: Tolerance::None,
            tolerance_plus: 0.0,
            tolerance_minus: 0.0,
            tolerance_height: 1.0,
            text_rotation: None,
        }
    }

    /// AutoCAD's own defaults, which hold for anything a dimension style in a file leaves out.
    pub fn autocad_defaults() -> DimensionLook {
        DimensionLook {
            text_height: 0.18,
            arrow_size: 0.18,
            extension_gap: 0.0625,
            extension_overshoot: 0.18,
            text_gap: 0.09,
            decimals: 4,
            ..DimensionLook::scaled(1.0)
        }
    }

    /// This look after its style changed from `old` to `new`: the sizes it took from the style follow the change,
    /// the ones set on the dimension itself stay. Text placed for the old sizes is laid out afresh.
    pub fn restyled(&self, old: &DimensionLook, new: &DimensionLook) -> DimensionLook {
        let mut look = self.clone();
        macro_rules! follow {
            ($($field:ident),*) => {$(
                if self.$field == old.$field {
                    look.$field = new.$field.clone();
                }
            )*};
        }
        follow!(
            scale,
            text_height,
            arrow_size,
            extension_gap,
            extension_overshoot,
            text_gap,
            decimals,
            angle_decimals,
            length_factor,
            arrowhead,
            trim_zeros,
            tolerance,
            tolerance_plus,
            tolerance_minus,
            tolerance_height
        );
        if look != *self {
            look.text_position = None;
        }
        look.style = new.style.clone();
        look
    }

    pub fn format(&self, distance: f64) -> String {
        self.with_override(self.length(distance))
    }

    /// A distance as the dimension writes it, before any text typed over it.
    pub fn length(&self, distance: f64) -> String {
        self.number(distance * self.length_factor, self.decimals)
    }

    /// An angle in degrees as the dimension writes it.
    pub fn angle(&self, degrees: f64) -> String {
        format!("{}°", self.number(degrees, self.angle_decimals))
    }

    fn number(&self, value: f64, decimals: u8) -> String {
        let mut measured = format!("{:.*}", decimals as usize, value);
        if self.trim_zeros && measured.contains('.') {
            measured = measured.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        measured
    }

    /// The text for a length, with its tolerance: the main text (after `prefix`, such as R for a radius), and the
    /// two lines stacked beside it for a deviation or limits tolerance.
    pub fn measured(&self, distance: f64, prefix: &str) -> (String, Option<(String, String)>) {
        let value = distance * self.length_factor;
        let size = |v: f64| self.number(v, self.decimals);
        match self.tolerance {
            Tolerance::None => (self.with_override(format!("{prefix}{}", size(value))), None),
            Tolerance::Symmetric => (self.with_override(format!("{prefix}{}±{}", size(value), size(self.tolerance_plus))), None),
            Tolerance::Deviation => {
                let minus = if self.tolerance_minus == 0.0 { size(0.0) } else { format!("-{}", size(self.tolerance_minus)) };
                (self.with_override(format!("{prefix}{}", size(value))), Some((format!("+{}", size(self.tolerance_plus)), minus)))
            }
            Tolerance::Limits => {
                let (upper, lower) = (size(value + self.tolerance_plus), size(value - self.tolerance_minus));
                (self.with_override(prefix.to_string()), Some((upper, lower)))
            }
        }
    }

    /// Where a dimension's text goes, around the middle of the whole: the main text, and the tolerance stack
    /// beside it. Each piece's anchor is the middle of its baseline.
    pub fn layout_text(
        &self,
        main: String,
        stack: Option<(String, String)>,
        middle: Point,
        angle: f64,
    ) -> (AnnotationText, Vec<AnnotationText>) {
        let height = self.text_height * self.scale;
        let stack_height = height * self.tolerance_height;
        let width = |text: &str, h: f64| text.chars().count() as f64 * h * 2.0 * HALF_CHARACTER_WIDTH;
        let main_width = width(&main, height);
        let stack_width = stack.as_ref().map_or(0.0, |(a, b)| width(a, stack_height).max(width(b, stack_height)));
        let gap = if main_width > 0.0 && stack_width > 0.0 { height * HALF_CHARACTER_WIDTH } else { 0.0 };
        let total = main_width + gap + stack_width;
        let (along, up) = (direction(angle), normal(direction(angle)));
        let place = |text: String, h: f64, at: Point| AnnotationText { text, height: h, anchor: at - up * (h / 2.0), middle: at, angle };
        let main_middle = middle - along * ((total - main_width) / 2.0);
        let mut pieces = vec![];
        if let Some((upper, lower)) = stack {
            let stack_middle = middle + along * ((total - stack_width) / 2.0);
            let apart = stack_height * 0.6;
            pieces.push(place(upper, stack_height, stack_middle + up * apart));
            pieces.push(place(lower, stack_height, stack_middle - up * apart));
        }
        (place(main, height, main_middle), pieces)
    }

    /// How far a dimension's text runs each side of its middle, its tolerance stack included.
    pub fn text_half_width(&self, main: &str, stack: &Option<(String, String)>) -> f64 {
        let height = self.text_height * self.scale;
        let main_width = main.chars().count() as f64 * height * HALF_CHARACTER_WIDTH;
        let stack_width = stack.as_ref().map_or(0.0, |(a, b)| a.chars().count().max(b.chars().count()) as f64)
            * height
            * self.tolerance_height
            * HALF_CHARACTER_WIDTH;
        main_width + stack_width + if stack.is_some() { height * HALF_CHARACTER_WIDTH / 2.0 } else { 0.0 }
    }

    /// The text shown for a measurement: the measurement, or the text typed over it with "<>" standing for it.
    pub fn with_override(&self, measured: String) -> String {
        if self.text.is_empty() { measured } else { self.text.replace("<>", &measured) }
    }
}

/// A named set of dimension sizes and units, as a drawing's dimension style table keeps it.
#[derive(Clone, Debug, PartialEq)]
pub struct DimensionStyle {
    pub name: String,
    pub look: DimensionLook,
    /// Dimensions in this style are annotative: their sizes are on paper, drawn at the annotation scale.
    pub annotative: bool,
}

impl DimensionStyle {
    pub fn standard() -> DimensionStyle {
        DimensionStyle { name: STANDARD_STYLE.into(), look: DimensionLook::default(), annotative: false }
    }
}

/// Everything a dimension draws, in drawing units.
#[derive(Clone, Debug, PartialEq)]
pub struct DimensionParts {
    pub extension_lines: [Segment; 2],
    pub dimension_line: Segment,
    pub arrows: Vec<[Point; 3]>, // filled triangles: arrowheads tip first, or the fans of dots
    pub ticks: Vec<Segment>,
    pub text: String,
    pub text_height: f64,
    pub text_anchor: Point, // middle of the text's baseline
    pub text_middle: Point, // middle of the text, its tolerance included
    pub text_angle: f64,    // degrees, kept readable (never upside down)
    /// The tolerance's two stacked lines beside the text, when it shows a deviation or limits.
    pub stack: Vec<AnnotationText>,
    /// The dimension line carried on out to text placed beyond the extension lines.
    pub line_extension: Option<Segment>,
}

impl DimensionParts {
    /// Every straight line drawn: extension lines, the dimension line and its extension, ticks.
    pub fn lines(&self) -> impl Iterator<Item = &Segment> {
        self.extension_lines.iter().chain([&self.dimension_line]).chain(&self.line_extension).chain(&self.ticks)
    }
}

/// Roughly how far text runs each side of its middle: AutoCAD's simplex digits are about 0.8 of the height apart.
pub(super) const HALF_CHARACTER_WIDTH: f64 = 0.4;

pub(super) fn normal(direction: Point) -> Point {
    pt(-direction.y, direction.x)
}

pub fn measurement(start: Point, end: Point, angle: f64) -> f64 {
    (end - start).dot(direction(angle)).abs()
}

pub(super) fn readable(angle: f64) -> f64 {
    let angle = modulo(angle, 360.0);
    if 90.0 < angle && angle <= 270.0 {
        angle - 180.0
    } else if angle > 270.0 {
        angle - 360.0
    } else {
        angle
    }
}

/// A triangle at the tip, pointing away from `toward`. A zero-length dimension falls back to the line direction.
fn arrow(tip: Point, toward: Point, direction: Point, normal: Point, length: f64) -> [Point; 3] {
    let run = toward - tip;
    let run_length = run.length();
    let unit = if run_length != 0.0 { run * (1.0 / run_length) } else { direction };
    let base = tip + unit * length;
    let half_width = length * ARROW_WIDTH_RATIO;
    [tip, base + normal * half_width, base + normal * -half_width]
}

/// A small filled dot at the end of the dimension line, as a fan of triangles.
fn dot(center: Point, radius: f64) -> Vec<[Point; 3]> {
    let corner = |i: usize| center + direction(360.0 * i as f64 / DOT_SIDES as f64) * radius;
    (0..DOT_SIDES).map(|i| [center, corner(i), corner(i + 1)]).collect()
}

/// What marks one end of a dimension line or leader whose tip is at `tip` and runs on along `inward` (a unit
/// vector): an arrowhead, a dot or a tick, as the look says.
pub(super) fn end_mark(tip: Point, inward: Point, look: &DimensionLook) -> (Vec<[Point; 3]>, Vec<Segment>) {
    let size = look.arrow_size * look.scale;
    match look.arrowhead {
        Arrowhead::Filled => (vec![arrow(tip, tip + inward, inward, normal(inward), size)], vec![]),
        Arrowhead::Dot => (dot(tip, size * DOT_RADIUS_RATIO), vec![]),
        Arrowhead::Tick => (vec![], vec![tick(tip, inward, normal(inward), size)]),
    }
}

/// An oblique stroke across the end of the dimension line, at 45 degrees to it.
fn tick(end: Point, along: Point, across: Point, size: f64) -> Segment {
    let slant = (along + across) * (size / 2.0);
    (end - slant, end + slant)
}

pub fn dimension_parts(start: Point, end: Point, line_point: Point, angle: f64, look: &DimensionLook) -> DimensionParts {
    let scale = look.scale;
    let along = direction(angle);
    let across = normal(along);
    let origins = [start, end];
    let offsets = origins.map(|origin| (line_point - origin).dot(across));
    let ends = [origins[0] + across * offsets[0], origins[1] + across * offsets[1]];
    let extension = |i: usize| {
        let side = if offsets[i] >= 0.0 { 1.0 } else { -1.0 };
        (origins[i] + across * (side * look.extension_gap * scale), ends[i] + across * (side * look.extension_overshoot * scale))
    };
    let size = look.arrow_size * scale;
    let (arrows, ticks) = match look.arrowhead {
        Arrowhead::Tick => (vec![], vec![tick(ends[0], along, across, size), tick(ends[1], along, across, size)]),
        Arrowhead::Dot => ([dot(ends[0], size * DOT_RADIUS_RATIO), dot(ends[1], size * DOT_RADIUS_RATIO)].concat(), vec![]),
        Arrowhead::Filled => (vec![arrow(ends[0], ends[1], along, across, size), arrow(ends[1], ends[0], along, across, size)], vec![]),
    };
    let text_angle = look.text_rotation.unwrap_or_else(|| readable(angle));
    let above = normal(direction(readable(angle)));
    let (text, stack) = look.measured(measurement(start, end, angle), "");
    let text_height = look.text_height * scale;
    let half_width = look.text_half_width(&text, &stack);
    let text_middle = look.text_position.unwrap_or_else(|| ends[0].midpoint(ends[1]) + above * (look.text_gap * scale + text_height / 2.0));
    // Text beyond either end takes the dimension line out to its far side.
    let along_text = (text_middle - ends[0]).dot(along);
    let along_end = (ends[1] - ends[0]).dot(along);
    let side = if along_text > along_end.max(0.0) {
        Some(1.0)
    } else if along_text < along_end.min(0.0) {
        Some(-1.0)
    } else {
        None
    };
    let line_extension = side.map(|side: f64| {
        let near = if (along_end >= 0.0) == (side > 0.0) { ends[1] } else { ends[0] };
        (near, ends[0] + along * (along_text + side * half_width))
    });
    let (main, stack) = look.layout_text(text, stack, text_middle, text_angle);
    DimensionParts {
        extension_lines: [extension(0), extension(1)],
        dimension_line: (ends[0], ends[1]),
        arrows,
        ticks,
        text: main.text,
        text_height,
        text_anchor: main.anchor,
        text_middle,
        text_angle,
        stack,
        line_extension,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_dimension_above_the_points() {
        let parts = dimension_parts(pt(0.0, 0.0), pt(10.0, 3.0), pt(5.0, 8.0), 0.0, &DimensionLook::default());
        assert_eq!(parts.text, "10.00");
        assert_eq!(parts.dimension_line, (pt(0.0, 8.0), pt(10.0, 8.0)));
        assert_eq!(parts.extension_lines[0], (pt(0.0, 0.625), pt(0.0, 9.25)));
        assert_eq!(parts.text_anchor, pt(5.0, 8.625));
        assert_eq!(parts.arrows[0][0], pt(0.0, 8.0));
        assert!((parts.arrows[0][1].x - 2.5).abs() < 1e-12);
        assert!(parts.ticks.is_empty());
    }

    #[test]
    fn text_is_never_upside_down() {
        let parts = dimension_parts(pt(0.0, 0.0), pt(-10.0, 0.0), pt(0.0, 5.0), 180.0, &DimensionLook::scaled(2.0));
        assert!(parts.text_angle.abs() < 1e-9);
        assert_eq!(parts.text_height, 5.0);
    }

    #[test]
    fn a_style_in_meters_draws_small() {
        let look = DimensionLook {
            text_height: 0.1,
            arrow_size: 0.05,
            arrowhead: Arrowhead::Tick,
            decimals: 3,
            text: "<> m".into(),
            ..DimensionLook::default()
        };
        let parts = dimension_parts(pt(0.0, 0.0), pt(2.5, 0.0), pt(0.0, 1.0), 0.0, &look);
        assert_eq!(parts.text, "2.500 m");
        assert_eq!(parts.text_height, 0.1);
        assert!(parts.arrows.is_empty());
        let (a, b) = parts.ticks[0];
        assert!((a.dist(b) - 0.05 * 2f64.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn trailing_zeros_can_be_left_off() {
        let look = DimensionLook { decimals: 3, length_factor: 1000.0, trim_zeros: true, ..DimensionLook::default() };
        assert_eq!(look.format(11.5), "11500");
        assert_eq!(look.format(0.9275), "927.5");
        let dots = DimensionLook { arrowhead: Arrowhead::Dot, ..DimensionLook::default() };
        let parts = dimension_parts(pt(0.0, 0.0), pt(10.0, 0.0), pt(0.0, 1.0), 0.0, &dots);
        assert_eq!(parts.arrows.len(), 2 * DOT_SIDES);
    }

    #[test]
    fn text_a_file_placed_beyond_the_extension_lines_takes_the_line_out_to_it() {
        let look = DimensionLook { text_position: Some(pt(20.0, 9.875)), ..DimensionLook::default() };
        let parts = dimension_parts(pt(0.0, 0.0), pt(10.0, 0.0), pt(5.0, 8.0), 0.0, &look);
        assert_eq!(parts.text_middle, pt(20.0, 9.875));
        assert_eq!(parts.text_anchor, pt(20.0, 8.625));
        let (near, far) = parts.line_extension.unwrap();
        assert_eq!(near, pt(10.0, 8.0));
        assert!((far.x - (20.0 + 5.0 * 2.5 * HALF_CHARACTER_WIDTH)).abs() < 1e-9 && far.y == 8.0);
        let centered = dimension_parts(pt(0.0, 0.0), pt(10.0, 0.0), pt(5.0, 8.0), 0.0, &DimensionLook::default());
        assert!(centered.line_extension.is_none());
    }

    #[test]
    fn a_style_change_carries_over_except_what_the_dimension_set_itself() {
        let old = DimensionLook::default();
        let new = DimensionLook { text_height: 0.2, arrow_size: 0.1, ..old.clone() };
        let plain = DimensionLook { text_position: Some(pt(5.0, 5.0)), ..old.clone() };
        let own_arrows = DimensionLook { arrow_size: 4.0, ..old.clone() };
        let restyled = plain.restyled(&old, &new);
        assert_eq!((restyled.text_height, restyled.arrow_size, restyled.text_position), (0.2, 0.1, None));
        let restyled = own_arrows.restyled(&old, &new);
        assert_eq!((restyled.text_height, restyled.arrow_size), (0.2, 4.0));
    }
}
