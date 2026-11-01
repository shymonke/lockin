//! Basic on-screen "Normal Calculator" in the style of computer-based-test calculators.
//! Quirks are intentional:
//! the final "=" follows BODMAS (2 + 3 × 4 = 14) although the screen shows 5 after "×"; "a % b" means
//! a × 0.01 × b; negative results show in brackets; MR then a digit appends to the recalled value;
//! only MS lights the M marker.

use crate::expr::{evaluate, js_to_number};

pub const KEYS: [&str; 27] = [
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", ".", "+", "-", "*", "/", "%", "sqrt", "reciprocal",
    "negate", "back", "clear", "equals", "MC", "MR", "MS", "M+", "M-",
];
/// Official check is `length > 8` before appending, so nine characters fit.
const MAX_INPUT: usize = 8;

/// Mirrors the official globals: `preview_val` is the result screen, `in_box1` the calculation line,
/// `in_box` the hidden entry box.
#[derive(Clone, Debug)]
pub struct Calc {
    ops: Vec<String>,
    current_val: String,
    preview_val: String,
    prev_operator: i32,
    memory_val: String,
    memory_shown: bool,
    in_box: String,
    in_box1: String,
    display_string: String,
    trig_display: String,
    stack_val1: i32,
    stack_val2: i32,
    bool_clear: bool,
    pending: bool,
}

#[derive(Debug, PartialEq)]
pub struct View<'a> {
    pub display: &'a str,
    pub expression: &'a str,
    pub memory: bool,
}

type R = Result<(), ()>;

fn binary_code(key: &str) -> Option<i32> {
    Some(match key {
        "+" => 1,
        "-" => 2,
        "*" => 3,
        "/" => 4,
        "%" => 9,
        _ => return None,
    })
}
fn eval_str(src: &str) -> Result<String, ()> {
    evaluate(src).map(|c| c.to_js_string())
}
/// Also wraps "Infinity", exactly as the original does.
fn wrap_complex(r: String) -> String {
    if r.contains('i') { format!("({r})") } else { r }
}
/// JavaScript `value != 0`.
fn loose_non_zero(v: &str) -> bool {
    js_to_number(v) != 0.0
}
fn or_else<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.is_empty() { if b.is_empty() { "0" } else { b } } else { a }
}

impl Default for Calc {
    fn default() -> Self {
        Calc {
            ops: Vec::new(),
            current_val: String::new(),
            preview_val: "0".into(),
            prev_operator: -2,
            memory_val: "0".into(),
            memory_shown: false,
            in_box: "0".into(),
            in_box1: String::new(),
            display_string: String::new(),
            trig_display: String::new(),
            stack_val1: 1,
            stack_val2: 0,
            bool_clear: true,
            pending: false,
        }
    }
}

impl Calc {
    /// Opening the official calculator clicks C first; memory survives closing and reopening.
    pub fn open(&mut self) {
        self.clear_all();
    }

    pub fn press(&mut self, key: &str) -> Result<(), String> {
        if !KEYS.contains(&key) {
            return Err(format!("Unknown calculator key: {key}"));
        }
        // The official jQuery handler stops at a thrown math.js error; earlier changes stay.
        let _ = self.handle(key);
        Ok(())
    }

    pub fn view(&self) -> View<'_> {
        View { display: &self.preview_val, expression: &self.in_box1, memory: self.memory_shown }
    }

    fn failed(&self) -> bool {
        self.in_box.contains("Infinity") || self.in_box.contains("Math Error")
    }

    fn handle(&mut self, key: &str) -> R {
        if key.len() == 1 && (key.as_bytes()[0].is_ascii_digit() || key == ".") {
            return self.digit(key);
        }
        if let Some(code) = binary_code(key) {
            return self.binary(key, code);
        }
        match key {
            "sqrt" | "reciprocal" | "negate" => self.unary(key),
            "equals" => {
                self.equals();
                Ok(())
            }
            "back" => {
                self.back();
                Ok(())
            }
            "clear" => {
                self.clear_all();
                Ok(())
            }
            _ => self.memory(key),
        }
    }

    fn clear_all(&mut self) {
        self.ops.clear();
        self.current_val.clear();
        self.prev_operator = -2;
        self.preview_val = "0".into();
        self.in_box = "0".into();
        self.in_box1.clear();
        self.display_string.clear();
        self.trig_display.clear();
        self.stack_val1 = 1;
        self.stack_val2 = 0;
        self.pending = false;
    }

    fn clear_if_infinite(&mut self) {
        if self.preview_val.contains("Infinity") || self.current_val.contains("Infinity") {
            self.clear_all();
        }
    }

    fn evaluate_ops(&self) -> String {
        eval_str(&self.ops.join(" ")).map(wrap_complex).unwrap_or_else(|_| "Math Error".into())
    }

    fn digit(&mut self, key: &str) -> R {
        if self.failed() {
            return Ok(());
        }
        if self.bool_clear {
            self.in_box = "0".into();
            self.bool_clear = false;
        }
        let s = self.in_box.clone();
        if s.len() > MAX_INPUT {
            return Ok(());
        }
        if key == "." && s.contains('.') {
            if !self.in_box1.is_empty() {
                self.in_box = "0.".into();
                self.in_box1.clear();
            }
            return Ok(());
        }
        if s.ends_with('i') {
            return Ok(());
        }
        if self.stack_val1 == 2 {
            self.in_box1.clear();
        } else if self.stack_val1 == 3 {
            let keep = self.in_box1.len().saturating_sub(self.trig_display.len());
            self.in_box1.truncate(keep);
            self.stack_val2 = 4;
        }
        self.clear_if_infinite();
        self.current_val = if self.prev_operator == -1 {
            if self.current_val == "0" { key.into() } else { format!("{}{key}", self.current_val) }
        } else {
            key.into()
        };
        self.preview_val = self.current_val.clone();
        self.prev_operator = -1;
        self.in_box = if loose_non_zero(&s) || s.len() > 1 || key == "." { format!("{s}{key}") } else { key.into() };
        self.stack_val1 = 1;
        Ok(())
    }

    fn binary(&mut self, key: &str, code: i32) -> R {
        if self.failed() {
            self.clear_all();
            return Ok(());
        }
        // Expression line (legacy display logic).
        if self.stack_val1 == 2 {
            self.in_box1.clear();
        }
        if self.stack_val1 == 0 {
            if !self.in_box1.contains("e+") {
                self.in_box1.pop();
            }
            self.stack_val2 = 2;
        }
        if self.stack_val1 == 5 || self.stack_val2 == 2 {
            self.stack_val2 = 0;
            self.display_string = format!("{}{key}", self.in_box1);
        } else {
            if self.in_box1.contains("e+0") {
                let with = if self.in_box.contains('-') { "e" } else { "e+" };
                self.in_box1 = self.in_box1.replacen("e+0", with, 1);
            }
            self.display_string = format!("{}{}{key}", self.in_box1, self.in_box);
        }
        // Value logic (new math.js stack).
        self.clear_if_infinite();
        if self.prev_operator > 0 && self.prev_operator < 21 {
            if self.prev_operator == 9 {
                let n = self.ops.len().saturating_sub(2);
                self.ops.truncate(n);
            }
            self.ops.pop();
        } else {
            let v = or_else(&self.current_val, &self.preview_val).to_string();
            self.ops.push(v);
        }
        let last = self.evaluate_ops();
        self.preview_val = last.clone();
        self.current_val = last;
        if code == 9 {
            self.ops.extend(["*".into(), "0.01".into(), "*".into()]);
        } else {
            self.ops.push(key.into());
        }
        self.prev_operator = code;
        self.in_box = self.preview_val.clone();
        self.stack_val1 = 0;
        self.bool_clear = true;
        self.pending = true;
        self.in_box1 = self.display_string.trim().to_string();
        Ok(())
    }

    fn unary(&mut self, key: &str) -> R {
        if self.failed() {
            self.clear_all();
            return Ok(());
        }
        let x = self.in_box.clone();
        if key == "negate" {
            self.clear_if_infinite();
            if self.ops.is_empty() {
                self.display_string.clear();
                self.in_box1.clear();
            }
            let mut r = eval_str(&format!("-1 * ({})", or_else(&self.current_val, &self.preview_val)))?;
            if r.contains('-') {
                r = format!("({r})");
            }
            self.current_val = r.clone();
            self.preview_val = r;
            self.prev_operator = -1;
            self.stack_val2 = 3;
        } else {
            let name = if key == "sqrt" { "sqrt" } else { "reciproc" };
            if self.stack_val2 == 2 || self.stack_val1 == 3 {
                if self.stack_val2 == 3 {
                    self.trig_display = format!("{name}({x})");
                    self.stack_val2 = 2;
                } else {
                    self.display_string = self.display_string.replacen(&self.trig_display, "", 1);
                    self.trig_display = format!("{name}({})", self.trig_display);
                }
            } else {
                if self.stack_val2 == 4 {
                    self.display_string.clear();
                }
                self.trig_display = format!("{name}({x})");
            }
            self.display_string.push_str(&self.trig_display);
            self.clear_if_infinite();
            let v = format!("({})", or_else(&self.preview_val, &self.current_val));
            let src = if key == "sqrt" { format!("sqrt({v})") } else { format!("1/({v})") };
            let r = eval_str(&src).unwrap_or_else(|_| "0".into());
            self.preview_val = wrap_complex(r);
            self.current_val.clear();
            self.prev_operator = if key == "sqrt" { 32 } else { 41 };
        }
        let shown = or_else(&self.preview_val, &self.current_val).to_string();
        if self.stack_val2 != 3 {
            self.stack_val2 = 2;
        }
        self.stack_val1 = 3;
        self.bool_clear = true;
        self.in_box = if js_to_number(&shown) == 0.0 { "0".into() } else { shown };
        self.in_box1 = self.display_string.clone();
        Ok(())
    }

    fn equals(&mut self) {
        let mut typed = self.preview_val.clone();
        self.clear_if_infinite();
        let v = or_else(&self.preview_val, "").to_string();
        self.ops.push(v);
        // On error the official shows the previous value and keeps the stack.
        if let Ok(r) = eval_str(&self.ops.join(" ")) {
            let mut r = wrap_complex(r);
            if r.starts_with('-') {
                r = format!("({r})");
            }
            self.preview_val = r;
            self.ops.clear();
        }
        self.current_val.clear();
        self.prev_operator = -2;
        if self.failed() {
            return;
        }
        if self.pending {
            self.in_box = self.preview_val.clone();
            self.bool_clear = true;
        }
        self.pending = false;
        self.display_string.clear();
        self.trig_display.clear();
        if self.stack_val1 != 2 {
            if (self.stack_val1 == 3 || self.stack_val2 == 1) && self.stack_val2 != 3 {
                typed.clear();
            }
            self.in_box1 = close_brackets(&format!("{}{typed}", self.in_box1));
        }
        self.stack_val1 = 2;
        self.stack_val2 = 0;
    }

    fn back(&mut self) {
        let shown = self.preview_val.clone();
        if self.stack_val1 == 1 || self.stack_val2 == 3 {
            if shown.len() > 1 {
                if self.failed() {
                    return;
                }
                self.in_box = shown[..shown.len() - 1].to_string();
                if self.in_box == "-" {
                    self.in_box = "0".into();
                }
            } else {
                self.in_box = "0".into();
            }
        }
        self.clear_if_infinite();
        if self.preview_val.len() > 1 && !self.preview_val.contains(')') {
            self.preview_val.pop();
            self.current_val = self.preview_val.clone();
        } else {
            self.preview_val = "0".into();
            self.current_val = "0".into();
            if self.ops.is_empty() {
                self.clear_all();
            }
        }
    }

    fn memory(&mut self, key: &str) -> R {
        if self.failed() {
            return Ok(());
        }
        self.clear_if_infinite();
        let shown = or_else(&self.preview_val, &self.current_val).to_string();
        match key {
            "MS" => {
                self.memory_val = shown;
                self.memory_shown = true;
            }
            "MR" => {
                self.preview_val = self.memory_val.clone();
                self.current_val = self.memory_val.clone();
                self.prev_operator = -1;
                self.stack_val1 = 1;
                self.stack_val2 = 0;
                self.in_box = self.preview_val.clone();
            }
            "MC" => {
                self.memory_val = "0".into();
                self.memory_shown = false;
            }
            _ => {
                let op = if key == "M+" { "+" } else { "-" };
                self.memory_val = wrap_complex(eval_str(&format!("{} {op} {shown}", self.memory_val))?);
            }
        }
        self.bool_clear = true;
        Ok(())
    }
}

fn close_brackets(text: &str) -> String {
    let open: i32 = text.chars().map(|c| match c {
        '(' => 1,
        ')' => -1,
        _ => 0,
    }).sum();
    if open > 0 { format!("{text}{}", ")".repeat(open as usize)) } else { text.to_string() }
}

#[cfg(test)]
mod tests {
    // Expected screens are the reference calculator's observed outputs.
    use super::*;

    fn run_from(mut c: Calc, keys: &[&str]) -> Calc {
        for k in keys {
            c.press(k).unwrap();
        }
        c
    }
    fn run(keys: &[&str]) -> Calc {
        let mut c = Calc::default();
        c.open();
        run_from(c, keys)
    }
    fn screen(keys: &[&str]) -> (String, String, bool) {
        let c = run(keys);
        let v = c.view();
        (v.display.to_string(), v.expression.to_string(), v.memory)
    }
    fn s(d: &str, e: &str, m: bool) -> (String, String, bool) {
        (d.into(), e.into(), m)
    }

    #[test]
    fn final_equals_follows_bodmas_while_screen_previews_left_to_right() {
        assert_eq!(screen(&["2", "+", "3", "*"]).0, "5");
        assert_eq!(screen(&["2", "+", "3", "*", "4", "equals"]), s("14", "2+3*4", false));
    }

    #[test]
    fn percent_is_binary_including_exam_quirk_after_plus() {
        assert_eq!(screen(&["1", "2", "%", "4", "8", "0", "0", "equals"]).0, "576");
        assert_eq!(screen(&["4", "8", "0", "0", "+", "8", "%", "equals"]), s("5184.64", "4800+8%4808", false));
    }

    #[test]
    fn entry_stops_at_nine_chars_and_negatives_show_in_brackets() {
        let digits: Vec<String> = "123456789123".chars().map(|c| c.to_string()).collect();
        let keys: Vec<&str> = digits.iter().map(String::as_str).collect();
        assert_eq!(screen(&keys).0, "123456789");
        assert_eq!(screen(&["3", "-", "8", "equals"]).0, "(-5)");
        assert_eq!(screen(&["5", "negate", "3", "equals"]), s("(-15)", "(-5)3", false));
    }

    #[test]
    fn memory_rules_and_reopen() {
        assert_eq!(
            screen(&["4", "MS", "+", "6", "equals", "M+", "MR", "2", "equals"]),
            s("142", "4+6142", true)
        );
        assert!(!screen(&["4", "M+"]).2);
        let mut reopened = run(&["7", "MS", "+", "1"]);
        reopened.open();
        assert_eq!(reopened.view(), View { display: "0", expression: "", memory: true });
        reopened.press("MR").unwrap();
        assert_eq!(reopened.view().display, "7");
    }

    #[test]
    fn division_by_zero_locks_digits_until_operator_or_clear() {
        let broken = run(&["5", "/", "0", "equals"]);
        assert_eq!(broken.view().display, "(Infinity)");
        assert_eq!(run_from(broken.clone(), &["7"]).view().display, "(Infinity)");
        assert_eq!(run_from(broken, &["+"]).view(), View { display: "0", expression: "", memory: false });
    }

    #[test]
    fn invalid_key_names_are_rejected() {
        assert!(Calc::default().press("^").is_err());
    }
}
