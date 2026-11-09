//! The arithmetic CAL evaluates: + - * / ^, parentheses, unary minus, decimals, pi and the functions sqrt, abs,
//! sin, cos and tan, whose angles are degrees as in AutoCAD's CAL.

/// Why an expression has no value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalcError {
    Invalid,
    DivisionByZero,
}

impl CalcError {
    /// What CAL tells the user.
    pub fn message(self) -> &'static str {
        match self {
            CalcError::Invalid => "Invalid expression.",
            CalcError::DivisionByZero => "Division by zero.",
        }
    }
}

/// The value of an expression such as `2*(3+4)^2` or `sqrt(2)/2`.
pub fn evaluate(expression: &str) -> Result<f64, CalcError> {
    let mut parser = Parser { text: expression.as_bytes(), at: 0 };
    let value = parser.sum()?;
    parser.skip_spaces();
    if parser.at != parser.text.len() || !value.is_finite() {
        return Err(CalcError::Invalid);
    }
    Ok(value)
}

struct Parser<'a> {
    text: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn skip_spaces(&mut self) {
        while self.text.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    /// Takes the next character if it is `wanted`.
    fn eat(&mut self, wanted: u8) -> bool {
        self.skip_spaces();
        let found = self.text.get(self.at) == Some(&wanted);
        if found {
            self.at += 1;
        }
        found
    }

    fn sum(&mut self) -> Result<f64, CalcError> {
        let mut value = self.product()?;
        loop {
            if self.eat(b'+') {
                value += self.product()?;
            } else if self.eat(b'-') {
                value -= self.product()?;
            } else {
                return Ok(value);
            }
        }
    }

    fn product(&mut self) -> Result<f64, CalcError> {
        let mut value = self.signed()?;
        loop {
            if self.eat(b'*') {
                value *= self.signed()?;
            } else if self.eat(b'/') {
                let divisor = self.signed()?;
                if divisor == 0.0 {
                    return Err(CalcError::DivisionByZero);
                }
                value /= divisor;
            } else {
                return Ok(value);
            }
        }
    }

    /// A unary minus binds looser than ^, so -2^2 is -4.
    fn signed(&mut self) -> Result<f64, CalcError> {
        if self.eat(b'-') {
            return Ok(-self.signed()?);
        }
        if self.eat(b'+') {
            return self.signed();
        }
        self.power()
    }

    /// ^ groups from the right: 2^3^2 is 2^9.
    fn power(&mut self) -> Result<f64, CalcError> {
        let base = self.atom()?;
        if self.eat(b'^') { Ok(base.powf(self.signed()?)) } else { Ok(base) }
    }

    fn atom(&mut self) -> Result<f64, CalcError> {
        if self.eat(b'(') {
            let value = self.sum()?;
            return if self.eat(b')') { Ok(value) } else { Err(CalcError::Invalid) };
        }
        self.skip_spaces();
        let start = self.at;
        let rest = &self.text[start..];
        if rest.first().is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
            let length = rest.iter().take_while(|c| c.is_ascii_digit() || **c == b'.').count();
            self.at += length;
            let digits = std::str::from_utf8(&rest[..length]).map_err(|_| CalcError::Invalid)?;
            return digits.parse().map_err(|_| CalcError::Invalid);
        }
        let length = rest.iter().take_while(|c| c.is_ascii_alphabetic()).count();
        if length == 0 {
            return Err(CalcError::Invalid);
        }
        self.at += length;
        let name = String::from_utf8_lossy(&rest[..length]).to_lowercase();
        if name == "pi" {
            return Ok(std::f64::consts::PI);
        }
        let function: fn(f64) -> f64 = match name.as_str() {
            "sqrt" => f64::sqrt,
            "abs" => f64::abs,
            "sin" => |degrees: f64| degrees.to_radians().sin(),
            "cos" => |degrees: f64| degrees.to_radians().cos(),
            "tan" => |degrees: f64| degrees.to_radians().tan(),
            _ => return Err(CalcError::Invalid),
        };
        if !self.eat(b'(') {
            return Err(CalcError::Invalid);
        }
        let argument = self.sum()?;
        if !self.eat(b')') {
            return Err(CalcError::Invalid);
        }
        Ok(function(argument))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> f64 {
        evaluate(text).unwrap_or_else(|e| panic!("{text}: {e:?}"))
    }

    #[test]
    fn precedence_and_parentheses() {
        assert_eq!(value("1+2*3"), 7.0);
        assert_eq!(value("(1+2)*3"), 9.0);
        assert_eq!(value("10-4-3"), 3.0, "left to right");
        assert_eq!(value("12/3/2"), 2.0);
        assert_eq!(value("2^3^2"), 512.0, "powers group from the right");
        assert_eq!(value("2*3^2"), 18.0);
        assert_eq!(value(" 1.5 + .5 "), 2.0);
        assert_eq!(value("((2))"), 2.0);
    }

    #[test]
    fn unary_minus() {
        assert_eq!(value("-3+5"), 2.0);
        assert_eq!(value("-2^2"), -4.0);
        assert_eq!(value("2^-1"), 0.5);
        assert_eq!(value("4*-2"), -8.0);
        assert_eq!(value("--3"), 3.0);
        assert_eq!(value("-(1+2)"), -3.0);
    }

    #[test]
    fn functions_take_degrees() {
        assert!((value("sin(30)") - 0.5).abs() < 1e-12);
        assert!((value("cos(60)") - 0.5).abs() < 1e-12);
        assert!((value("tan(45)") - 1.0).abs() < 1e-12);
        assert_eq!(value("sqrt(16)"), 4.0);
        assert_eq!(value("ABS(-2.5)"), 2.5);
        assert_eq!(value("2*pi"), 2.0 * std::f64::consts::PI);
        assert_eq!(value("sqrt(9)+abs(-1)*2"), 5.0);
    }

    #[test]
    fn division_by_zero_and_bad_input() {
        assert_eq!(evaluate("1/0"), Err(CalcError::DivisionByZero));
        assert_eq!(evaluate("1/(2-2)"), Err(CalcError::DivisionByZero));
        for text in ["", "1+", "(1+2", "1+2)", "2**3", "foo(2)", "sqrt 4", "1.2.3", "3 4", "sqrt(-1)", "abc"] {
            assert_eq!(evaluate(text), Err(CalcError::Invalid), "{text}");
        }
    }
}
