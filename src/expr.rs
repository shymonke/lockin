//! Tiny math.js-compatible evaluator for the calculator.
//! Covers exactly what the calculator feeds it: numbers, `i`, `Infinity`, `NaN`, `sqrt(..)`,
//! `+ - * /`, unary minus, parentheses and implicit multiplication (`(-5)3`, `2i`).
//! Real results format like JavaScript `Number#toString`; complex ones like math.js `Complex#format`.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct C {
    pub re: f64,
    pub im: f64,
}

impl C {
    const fn real(re: f64) -> Self {
        C { re, im: 0.0 }
    }
    fn is_real(self) -> bool {
        self.im == 0.0
    }
    fn add(self, o: C) -> C {
        C { re: self.re + o.re, im: self.im + o.im }
    }
    fn sub(self, o: C) -> C {
        C { re: self.re - o.re, im: self.im - o.im }
    }
    fn mul(self, o: C) -> C {
        if self.is_real() && o.is_real() {
            return C::real(self.re * o.re);
        }
        C { re: self.re * o.re - self.im * o.im, im: self.re * o.im + self.im * o.re }
    }
    fn div(self, o: C) -> C {
        if self.is_real() && o.is_real() {
            return C::real(self.re / o.re);
        }
        let d = o.re * o.re + o.im * o.im;
        C { re: (self.re * o.re + self.im * o.im) / d, im: (self.im * o.re - self.re * o.im) / d }
    }
    fn sqrt(self) -> C {
        if self.is_real() {
            return if self.re >= 0.0 || self.re.is_nan() {
                C::real(self.re.sqrt())
            } else {
                C { re: 0.0, im: (-self.re).sqrt() }
            };
        }
        let r = (self.re * self.re + self.im * self.im).sqrt();
        let re = ((r + self.re) / 2.0).sqrt();
        let im = ((r - self.re) / 2.0).sqrt().copysign(self.im);
        C { re, im }
    }
    /// `evaluate(..).toString()` text.
    pub fn to_js_string(self) -> String {
        if self.is_real() {
            return js_number(self.re);
        }
        let re = mathjs_format(self.re);
        if self.re == 0.0 {
            return match self.im {
                x if x == 1.0 => "i".into(),
                x if x == -1.0 => "-i".into(),
                x => format!("{}i", mathjs_format(x)),
            };
        }
        if self.im < 0.0 {
            if self.im == -1.0 { format!("{re} - i") } else { format!("{re} - {}i", mathjs_format(-self.im)) }
        } else if self.im == 1.0 {
            format!("{re} + i")
        } else {
            format!("{re} + {}i", mathjs_format(self.im))
        }
    }
}

/// JavaScript `Number.prototype.toString()` for f64.
pub fn js_number(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if x == 0.0 {
        return "0".into();
    }
    let sign = if x < 0.0 { "-" } else { "" };
    // Shortest round-trip digits via Rust's `{:e}` ("1.2345e-7").
    let e = format!("{:e}", x.abs());
    let (mant, exp) = e.split_once('e').unwrap();
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp.parse::<i32>().unwrap() + 1; // value = 0.d1d2.. × 10^n
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let es = if e >= 0 { format!("e+{e}") } else { format!("e-{}", -e) };
        if k == 1 { format!("{digits}{es}") } else { format!("{}.{}{es}", &digits[..1], &digits[1..]) }
    };
    format!("{sign}{body}")
}

/// math.js `format(number)` in "auto" notation: exponential below 1e-3 or from 1e5.
fn mathjs_format(x: f64) -> String {
    let a = x.abs();
    if x == 0.0 || !x.is_finite() || (1e-3..1e5).contains(&a) {
        return js_number(x);
    }
    let e = format!("{:e}", x);
    let (mant, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    if exp >= 0 { format!("{mant}e+{exp}") } else { format!("{mant}e{exp}") }
}

/// JavaScript `Number(text)` (only the forms the calculator produces).
pub fn js_to_number(text: &str) -> f64 {
    let t = text.trim();
    if t.is_empty() {
        return 0.0;
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if t.bytes().any(|b| b.is_ascii_alphabetic() && b != b'e' && b != b'E') {
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(u8),
    LParen,
    RParen,
}

fn lex(src: &str) -> Result<Vec<Tok>, ()> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b' ' => i += 1,
            b'+' | b'-' | b'*' | b'/' => {
                out.push(Tok::Op(c));
                i += 1;
            }
            b'(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            b')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            b'0'..=b'9' | b'.' => {
                let start = i;
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                    i += 1;
                }
                // Exponent only when followed by digits (so `2e` is not swallowed).
                if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
                    let mut j = i + 1;
                    if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
                        j += 1;
                    }
                    if j < b.len() && b[j].is_ascii_digit() {
                        while j < b.len() && b[j].is_ascii_digit() {
                            j += 1;
                        }
                        i = j;
                    }
                }
                let text = &src[start..i];
                if text == "." || text.matches('.').count() > 1 {
                    return Err(());
                }
                out.push(Tok::Num(text.parse().map_err(|_| ())?));
            }
            c if c.is_ascii_alphabetic() => {
                let start = i;
                while i < b.len() && b[i].is_ascii_alphanumeric() {
                    i += 1;
                }
                out.push(Tok::Ident(src[start..i].to_string()));
            }
            _ => return Err(()),
        }
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }
    fn expr(&mut self) -> Result<C, ()> {
        let mut v = self.term()?;
        while let Some(Tok::Op(op @ (b'+' | b'-'))) = self.peek().cloned() {
            self.pos += 1;
            let r = self.term()?;
            v = if op == b'+' { v.add(r) } else { v.sub(r) };
        }
        Ok(v)
    }
    fn term(&mut self) -> Result<C, ()> {
        let mut v = self.unary()?;
        while let Some(Tok::Op(op @ (b'*' | b'/'))) = self.peek().cloned() {
            self.pos += 1;
            let r = self.unary()?;
            v = if op == b'*' { v.mul(r) } else { v.div(r) };
        }
        Ok(v)
    }
    fn unary(&mut self) -> Result<C, ()> {
        match self.peek() {
            Some(Tok::Op(b'-')) => {
                self.pos += 1;
                Ok(C::real(-1.0).mul(self.unary()?))
            }
            Some(Tok::Op(b'+')) => {
                self.pos += 1;
                self.unary()
            }
            _ => self.implicit(),
        }
    }
    fn implicit(&mut self) -> Result<C, ()> {
        let mut v = self.primary()?;
        while matches!(self.peek(), Some(Tok::Num(_) | Tok::Ident(_) | Tok::LParen)) {
            v = v.mul(self.primary()?);
        }
        Ok(v)
    }
    fn primary(&mut self) -> Result<C, ()> {
        match self.next().ok_or(())? {
            Tok::Num(n) => Ok(C::real(n)),
            Tok::LParen => {
                let v = self.expr()?;
                match self.next() {
                    Some(Tok::RParen) => Ok(v),
                    _ => Err(()),
                }
            }
            Tok::Ident(name) => match name.as_str() {
                "i" => Ok(C { re: 0.0, im: 1.0 }),
                "Infinity" => Ok(C::real(f64::INFINITY)),
                "NaN" => Ok(C::real(f64::NAN)),
                "sqrt" => {
                    if self.next() != Some(Tok::LParen) {
                        return Err(());
                    }
                    let v = self.expr()?;
                    if self.next() != Some(Tok::RParen) {
                        return Err(());
                    }
                    Ok(v.sqrt())
                }
                _ => Err(()),
            },
            _ => Err(()),
        }
    }
}

/// `math.evaluate(src)`; `Err` where math.js throws.
pub fn evaluate(src: &str) -> Result<C, ()> {
    let mut p = Parser { toks: lex(src)?, pos: 0 };
    if p.toks.is_empty() {
        return Err(());
    }
    let v = p.expr()?;
    if p.pos != p.toks.len() {
        return Err(());
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_number_matches_javascript() {
        let cases = [
            (14.0, "14"),
            (5184.64, "5184.64"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1e21, "1e+21"),
            (123456789012345680000.0, "123456789012345680000"),
            (1e-7, "1e-7"),
            (0.000001, "0.000001"),
            (-2.5e-9, "-2.5e-9"),
            (1.5e300, "1.5e+300"),
        ];
        for (x, s) in cases {
            assert_eq!(js_number(x), s);
        }
    }

    #[test]
    fn evaluates_precedence_implicit_and_complex() {
        assert_eq!(evaluate("2 + 3 * 4").unwrap().to_js_string(), "14");
        assert_eq!(evaluate("(-5)3").unwrap().to_js_string(), "-15");
        assert_eq!(evaluate("12 * 0.01 * 4800").unwrap().to_js_string(), "576");
        assert_eq!(evaluate("sqrt((-4))").unwrap().to_js_string(), "2i");
        assert_eq!(evaluate("(2i) + 3").unwrap().to_js_string(), "3 + 2i");
        assert_eq!(evaluate("5 / 0").unwrap().to_js_string(), "Infinity");
        assert!(evaluate("2 +").is_err());
        assert!(evaluate("(1").is_err());
    }
}
