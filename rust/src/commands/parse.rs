use crate::model::geometry::{Point, direction, pt};

/// Any finite number, as Python's float() reads it. Angles are read as degrees.
pub fn parse_finite(text: &str) -> Option<f64> {
    let text = text.trim();
    let value: f64 = text.parse().ok()?;
    value.is_finite().then_some(value)
}

/// A finite number greater than zero. The decimal separator is always a dot.
pub fn parse_number(text: &str) -> Option<f64> {
    parse_finite(text).filter(|value| *value > 0.0)
}

/// A finite number, zero or greater.
pub fn parse_length(text: &str) -> Option<f64> {
    parse_finite(text).filter(|value| *value >= 0.0)
}

/// Accepts x,y  and  @dx,dy  and  @distance<angle  (relative to base, or the origin without one).
pub fn parse_point(text: &str, base: Option<Point>) -> Option<Point> {
    let text = text.trim();
    let (relative, body) = match text.strip_prefix('@') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let offset = parse_offset(body)?;
    let origin = if relative { base.unwrap_or_default() } else { pt(0.0, 0.0) };
    Some(origin + offset)
}

fn parse_offset(body: &str) -> Option<Point> {
    if let Some((distance, angle)) = body.split_once('<') {
        let (distance, angle) = (parse_finite(distance)?, parse_finite(angle)?);
        return Some(direction(angle) * distance);
    }
    let parts: Vec<&str> = body.split(',').collect();
    if parts.len() != 2 {
        return None;
    }
    Some(pt(parse_finite(parts[0])?, parse_finite(parts[1])?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_and_relative_points() {
        assert_eq!(parse_point("3,4", Some(pt(10.0, 10.0))), Some(pt(3.0, 4.0)));
        assert_eq!(parse_point(" @1,-2 ", Some(pt(10.0, 10.0))), Some(pt(11.0, 8.0)));
        assert_eq!(parse_point("@1,1", None), Some(pt(1.0, 1.0)));
        let polar = parse_point("@2<90", Some(pt(1.0, 1.0))).unwrap();
        assert!(polar.dist(pt(1.0, 3.0)) < 1e-12);
    }

    #[test]
    fn rubbish_is_refused() {
        for text in ["", "1", "1,2,3", "a,b", "@<45", "inf,1", "nan,2"] {
            assert_eq!(parse_point(text, None), None, "{text}");
        }
    }

    #[test]
    fn numbers_and_lengths() {
        assert_eq!(parse_number("2.5"), Some(2.5));
        assert_eq!(parse_number("0"), None);
        assert_eq!(parse_length("0"), Some(0.0));
        assert_eq!(parse_length("-1"), None);
        assert_eq!(parse_finite("-30"), Some(-30.0));
        assert_eq!(parse_finite("1e3"), Some(1000.0));
    }
}

/// Dynamic input: what typed into the fields by the cursor means on the command line. With a base point the fields
/// are a length and an angle, so `50` then `30` is `@50<30`; a length alone goes along the cursor's direction
/// (direct distance entry). With no base point they are X and Y. None when what is typed is to be used as it is.
pub fn dynamic_entry(typed: &str, first_field: Option<&str>, base: Option<Point>, cursor: Option<Point>) -> Option<String> {
    let number = |text: &str| parse_finite(text.trim());
    let toward = || match (base, cursor) {
        (Some(base), Some(cursor)) if cursor != base => Some(base.angle_to(cursor)),
        _ => None,
    };
    match first_field.and_then(number) {
        Some(first) => match base {
            Some(_) => {
                let angle = number(typed).or_else(toward).unwrap_or(0.0);
                Some(format!("@{first}<{angle}"))
            }
            None => Some(format!("{first},{}", number(typed)?)),
        },
        None => {
            let length = number(typed)?;
            base?;
            Some(format!("@{length}<{}", toward()?))
        }
    }
}

#[cfg(test)]
mod dynamic_tests {
    use super::*;
    use crate::model::geometry::pt;

    #[test]
    fn fields_become_polar_or_absolute_points() {
        let base = Some(pt(0.0, 0.0));
        assert_eq!(dynamic_entry("30", Some("50"), base, None).as_deref(), Some("@50<30"));
        assert_eq!(dynamic_entry("20", Some("10"), None, None).as_deref(), Some("10,20"));
        assert_eq!(dynamic_entry("25", None, base, Some(pt(0.0, 7.0))).as_deref(), Some("@25<90"), "along the cursor");
        assert_eq!(dynamic_entry("5,5", None, base, Some(pt(1.0, 0.0))), None, "coordinates stay as typed");
        assert_eq!(dynamic_entry("25", None, None, Some(pt(1.0, 0.0))), None, "a number alone needs a base point");
    }
}
