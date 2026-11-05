//! MTEXT's formatting: reading its codes into runs of styled text, writing them back, and laying them out. Codes
//! not shown here (\A, \Q, \T, \W, \O) are read past. A stacked run holds "upper/lower", or "upper^lower" with no bar.

use super::geometry::{Point, direction};

/// How a run of text differs from the text object it is in.
#[derive(Clone, Debug, PartialEq)]
pub struct RunStyle {
    /// A font of its own; None takes the object's.
    pub font: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// Height per the object's height.
    pub height: f64,
    /// A color of its own (as an entity's color); None takes the object's.
    pub color: Option<i32>,
    pub stack: bool,
}

impl Default for RunStyle {
    fn default() -> Self {
        RunStyle { font: None, bold: false, italic: false, underline: false, height: 1.0, color: None, stack: false }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub text: String,
    pub style: RunStyle,
}

/// Lines of runs.
pub type RichText = Vec<Vec<Run>>;

/// The codes of an MTEXT read into lines of runs. `height` is the object's, which absolute heights are taken against.
pub fn parse(raw: &str, height: f64) -> RichText {
    let mut lines: RichText = vec![vec![]];
    let mut style = RunStyle::default();
    let mut scopes: Vec<RunStyle> = vec![];
    let mut text = String::new();
    let flush = |text: &mut String, style: &RunStyle, lines: &mut RichText| {
        if !text.is_empty() {
            let run = Run { text: std::mem::take(text), style: style.clone() };
            if let Some(last) = lines.last_mut() {
                match last.last_mut() {
                    Some(previous) if previous.style == run.style && !run.style.stack => previous.text.push_str(&run.text),
                    _ => last.push(run),
                }
            }
        }
    };
    let chars: Vec<char> = raw.chars().collect();
    let mut at = 0;
    // The text up to the ';' that ends a code's value, and the place after it.
    let until_semicolon = |from: usize| -> (String, usize) {
        let end = chars[from..].iter().position(|c| *c == ';').map_or(chars.len(), |i| from + i);
        (chars[from..end].iter().collect(), (end + 1).min(chars.len()))
    };
    while at < chars.len() {
        let c = chars[at];
        at += 1;
        match c {
            '{' => {
                flush(&mut text, &style, &mut lines);
                scopes.push(style.clone());
            }
            '}' => {
                flush(&mut text, &style, &mut lines);
                if let Some(outer) = scopes.pop() {
                    style = outer;
                }
            }
            '\\' => {
                let Some(code) = chars.get(at).copied() else { break };
                at += 1;
                match code {
                    'P' => {
                        flush(&mut text, &style, &mut lines);
                        lines.push(vec![]);
                    }
                    '~' => text.push(' '),
                    '\\' | '{' | '}' => text.push(code),
                    'L' | 'l' => {
                        flush(&mut text, &style, &mut lines);
                        style.underline = code == 'L';
                    }
                    'O' | 'o' | 'K' | 'k' | 'N' => {}
                    'f' | 'F' => {
                        flush(&mut text, &style, &mut lines);
                        let (value, next) = until_semicolon(at);
                        at = next;
                        let mut parts = value.split('|');
                        let name = parts.next().unwrap_or("").trim().to_string();
                        style.font = (!name.is_empty()).then_some(name);
                        style.bold = false;
                        style.italic = false;
                        for part in parts {
                            match part {
                                "b1" => style.bold = true,
                                "i1" => style.italic = true,
                                _ => {}
                            }
                        }
                    }
                    'H' => {
                        flush(&mut text, &style, &mut lines);
                        let (value, next) = until_semicolon(at);
                        at = next;
                        let value = value.trim();
                        if let Some(factor) = value.strip_suffix(['x', 'X']).and_then(|v| v.parse::<f64>().ok()) {
                            style.height *= factor;
                        } else if let Ok(absolute) = value.parse::<f64>()
                            && height > 0.0
                        {
                            style.height = absolute / height;
                        }
                    }
                    'C' | 'c' => {
                        flush(&mut text, &style, &mut lines);
                        let (value, next) = until_semicolon(at);
                        at = next;
                        if let Ok(number) = value.trim().parse::<i64>() {
                            style.color = if code == 'C' {
                                (number != 256 && number != 0).then_some(number as i32)
                            } else {
                                // \c gives 0xBBGGRR.
                                Some(crate::canvas::aci::true_color(number as u8, (number >> 8) as u8, (number >> 16) as u8))
                            };
                        }
                    }
                    'S' => {
                        flush(&mut text, &style, &mut lines);
                        let (value, next) = until_semicolon(at);
                        at = next;
                        // A caret in a DXF file is written with a space after it.
                        let shown = value.replace('#', "/").replace("\\/", "/").replace("\\^", "^").replace("^ ", "^");
                        let stacked = RunStyle { stack: true, ..style.clone() };
                        flush(&mut shown.clone(), &stacked, &mut lines);
                    }
                    'A' | 'Q' | 'T' | 'W' | 'p' => at = until_semicolon(at).1,
                    other => {
                        text.push('\\');
                        text.push(other);
                    }
                }
            }
            other => text.push(other),
        }
    }
    flush(&mut text, &style, &mut lines);
    // %%d and the like read as the characters they stand for.
    for run in lines.iter_mut().flatten() {
        run.text = crate::io::dxf::decode_percent_codes(&run.text);
    }
    lines
}

/// The text with no formatting, one '\n' between lines.
pub fn plain(lines: &RichText) -> String {
    lines.iter().map(|line| line.iter().map(|run| run.text.as_str()).collect::<String>()).collect::<Vec<_>>().join("\n")
}

/// Whether any run has a look of its own: if not, plain text says it all.
pub fn has_formatting(lines: &RichText) -> bool {
    lines.iter().flatten().any(|run| run.style != RunStyle::default())
}

/// The runs of a text object's formatting, as long as they still spell its text: other ways of changing the text
/// leave them behind, and then the plain text is what counts.
pub fn valid_rich(text: &str, formatted: &str, height: f64) -> Option<RichText> {
    if formatted.is_empty() {
        return None;
    }
    let lines = parse(formatted, height);
    (plain(&lines) == text && has_formatting(&lines)).then_some(lines)
}

/// Lines of runs as MTEXT codes.
pub fn serialize(lines: &RichText) -> String {
    let escape = |text: &str| text.replace('\\', "\\\\").replace('{', "\\{").replace('}', "\\}");
    let mut out = vec![];
    for line in lines {
        let mut codes = String::new();
        for run in line {
            let body = if run.style.stack { format!("\\S{};", run.text.replace('^', "^ ")) } else { escape(&run.text) };
            if (RunStyle { stack: false, ..run.style.clone() }) == RunStyle::default() {
                codes.push_str(&body);
                continue;
            }
            codes.push('{');
            let style = &run.style;
            if style.font.is_some() || style.bold || style.italic {
                codes.push_str(&format!(
                    "\\f{}|b{}|i{}|c0|p0;",
                    style.font.as_deref().unwrap_or(""),
                    i32::from(style.bold),
                    i32::from(style.italic)
                ));
            }
            if style.height != 1.0 {
                codes.push_str(&format!("\\H{}x;", style.height));
            }
            if let Some(color) = style.color {
                if crate::canvas::aci::is_true_color(color) {
                    let (r, g, b) = crate::canvas::aci::color_rgb(color).unwrap_or((0, 0, 0));
                    codes.push_str(&format!("\\c{};", ((b as i64) << 16) | ((g as i64) << 8) | r as i64));
                } else {
                    codes.push_str(&format!("\\C{color};"));
                }
            }
            if style.underline {
                codes.push_str("\\L");
            }
            codes.push_str(&body);
            if style.underline {
                codes.push_str("\\l");
            }
            codes.push('}');
        }
        out.push(codes);
    }
    out.join("\\P")
}

/// A run placed: its left end on the baseline, and how tall.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedRun {
    pub origin: Point,
    pub text: String,
    pub height: f64,
    pub style: RunStyle,
}

/// Stacked text's height per its run's.
const STACK_SCALE: f64 = 0.7;

/// Runs laid out line by line, given how to measure a run (text, height, style) along the baseline: `origins` turns
/// the lines' widths into where each starts. Also the lines to draw: underlines and fraction bars.
pub fn layout(
    lines: &RichText,
    height: f64,
    rotation: f64,
    origins: impl Fn(&[f64]) -> Vec<Point>,
    measure: &dyn Fn(&str, f64, &RunStyle) -> f64,
) -> (Vec<PlacedRun>, Vec<[Point; 2]>) {
    let (along, up) = (direction(rotation), direction(rotation + 90.0));
    let stack_parts = |text: &str| -> (String, String, bool) {
        match text.split_once('/') {
            Some((a, b)) => (a.to_string(), b.to_string(), true),
            None => match text.split_once('^') {
                Some((a, b)) => (a.to_string(), b.to_string(), false),
                None => (text.to_string(), String::new(), false),
            },
        }
    };
    let run_width = |run: &Run| -> f64 {
        let h = height * run.style.height;
        if run.style.stack {
            let (upper, lower, _) = stack_parts(&run.text);
            measure(&upper, h * STACK_SCALE, &run.style).max(measure(&lower, h * STACK_SCALE, &run.style))
        } else {
            measure(&run.text, h, &run.style)
        }
    };
    let widths: Vec<f64> = lines.iter().map(|line| line.iter().map(run_width).sum()).collect();
    let starts = origins(&widths);
    let (mut placed, mut rules) = (vec![], vec![]);
    for (line, start) in lines.iter().zip(starts) {
        let mut x = 0.0;
        for run in line {
            let h = height * run.style.height;
            let width = run_width(run);
            let at = start + along * x;
            if run.style.stack {
                let (upper, lower, bar) = stack_parts(&run.text);
                let small = h * STACK_SCALE;
                let plain_style = RunStyle { stack: false, ..run.style.clone() };
                let centered = |text: &str| (width - measure(text, small, &run.style)) / 2.0;
                placed.push(PlacedRun {
                    origin: at + along * centered(&upper) + up * (h * 0.55),
                    text: upper.clone(),
                    height: small,
                    style: plain_style.clone(),
                });
                placed.push(PlacedRun {
                    origin: at + along * centered(&lower) - up * (h * 0.3),
                    text: lower.clone(),
                    height: small,
                    style: plain_style,
                });
                if bar {
                    rules.push([at + up * (h * 0.45), at + along * width + up * (h * 0.45)]);
                }
            } else {
                placed.push(PlacedRun { origin: at, text: run.text.clone(), height: h, style: run.style.clone() });
            }
            if run.style.underline {
                rules.push([at - up * (h * 0.15), at + along * width - up * (h * 0.15)]);
            }
            x += width;
        }
    }
    (placed, rules)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::geometry::pt;

    #[test]
    fn codes_read_into_styled_runs_and_back() {
        let raw = "Plain {\\fArial|b1|i0|c0|p0;bold} {\\H2x;\\C1;big}\\P{\\L\\fTimes|b0|i1;under\\l} \\S1/2;";
        let lines = parse(raw, 2.5);
        assert_eq!(plain(&lines), "Plain bold big\nunder 1/2");
        let bold = &lines[0][1];
        assert_eq!((bold.text.as_str(), bold.style.bold, bold.style.font.as_deref()), ("bold", true, Some("Arial")));
        let big = lines[0].iter().find(|r| r.text == "big").unwrap();
        assert_eq!((big.style.height, big.style.color), (2.0, Some(1)));
        let under = &lines[1][0];
        assert!(under.style.underline && under.style.italic);
        assert!(lines[1].last().unwrap().style.stack);
        let again = parse(&serialize(&lines), 2.5);
        assert_eq!(again, lines);
    }

    #[test]
    fn absolute_heights_and_unknown_codes() {
        let lines = parse("\\A1;\\H5;tall\\Pnext{\\Q15;\\W1.2;x}", 2.5);
        assert_eq!(lines[0][0].style.height, 2.0);
        assert_eq!(plain(&lines), "tall\nnextx");
        assert!(!has_formatting(&parse("just\\Ptext", 2.5)));
        let stacked = parse("\\S+0.1^ -0.2;", 2.5);
        assert_eq!(plain(&stacked), "+0.1^-0.2");
        assert_eq!(serialize(&stacked), "\\S+0.1^ -0.2;");
    }

    #[test]
    fn formatting_counts_only_while_it_spells_the_text() {
        let raw = "{\\fArial|b1;Hello}";
        assert!(valid_rich("Hello", raw, 1.0).is_some());
        assert!(valid_rich("Changed", raw, 1.0).is_none());
    }

    #[test]
    fn runs_follow_one_another_and_stacks_sit_one_over_the_other() {
        let lines = parse("ab{\\L cd}\\S1/2;", 1.0);
        let (placed, rules) = layout(&lines, 10.0, 0.0, |_| vec![pt(0.0, 0.0)], &|t: &str, h, _: &RunStyle| t.chars().count() as f64 * h);
        assert_eq!(placed[0].origin, pt(0.0, 0.0));
        assert_eq!(placed[1].origin, pt(20.0, 0.0));
        assert_eq!(placed.len(), 4);
        assert!(placed[2].origin.y > placed[3].origin.y);
        assert_eq!(placed[2].height, 7.0);
        assert_eq!(rules.len(), 2, "an underline and a fraction bar");
    }
}
