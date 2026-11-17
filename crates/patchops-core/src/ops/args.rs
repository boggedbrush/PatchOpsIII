//! Argument validation mimicking the legacy pydantic-v2 (lax mode) models.
//!
//! * missing required field / wrong type / out of range -> [`OpError::Validation`]
//! * unknown extra keys are ignored
//! * `args` may be absent (`null`) = `{}`
//! * bools accept `true/false`, `0/1`, and the strings pydantic accepts (`"yes"`, `"off"`, ...);
//!   anything else (e.g. `"maybe"`) is a validation error
//! * ints accept integers, integral floats and numeric strings
//! * strings are strict (numbers are *not* coerced to strings)

use serde_json::{Map, Value};

use crate::config_ini::ConfigValue;
use crate::error::OpError;
use crate::pyfmt::{number_is_int, py_int};

pub struct Args<'a> {
    obj: Option<&'a Map<String, Value>>,
}

fn invalid(field: &str, msg: &str) -> OpError {
    OpError::Validation(format!("{field}: {msg}"))
}

/// pydantic-core's `str -> int` rule (see [`Args::as_int`]); `None` when it is not an integer.
fn lax_int_str(s: &str) -> Option<i128> {
    let t = s.trim();
    let (neg, body) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let (int_part, frac) = match body.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (body, None),
    };
    if frac.is_some_and(|f| f.is_empty() || !f.bytes().all(|c| c == b'0')) {
        return None;
    }
    // one `_` between two digits at most; no leading/trailing `_`
    let b = int_part.as_bytes();
    if b.is_empty() || !b.iter().all(|c| c.is_ascii_digit() || *c == b'_') {
        return None;
    }
    for (i, &c) in b.iter().enumerate() {
        if c == b'_' && !(i > 0 && b[i - 1].is_ascii_digit() && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            return None;
        }
    }
    let digits: String = int_part.chars().filter(|c| *c != '_').collect();
    let mag = digits.parse::<u128>().ok().filter(|m| *m <= i128::MAX as u128).map_or(i128::MAX, |m| m as i128);
    Some(if neg { -mag } else { mag })
}

impl<'a> Args<'a> {
    /// `null` is treated as `{}`; any other non-object is a validation error.
    pub fn new(v: &'a Value) -> Result<Args<'a>, OpError> {
        match v {
            Value::Null => Ok(Args { obj: None }),
            Value::Object(m) => Ok(Args { obj: Some(m) }),
            _ => Err(OpError::Validation("Input should be a valid dictionary or object".into())),
        }
    }

    /// Raw value; `None` when missing (JSON `null` is returned as `Some(Value::Null)`).
    pub fn raw(&self, name: &str) -> Option<&'a Value> {
        self.obj.and_then(|m| m.get(name))
    }

    fn present(&self, name: &str) -> Option<&'a Value> {
        self.raw(name)
    }

    // ---- strings -------------------------------------------------------------------------

    fn as_str(name: &str, v: &Value) -> Result<String, OpError> {
        match v {
            Value::String(s) => Ok(s.clone()),
            _ => Err(invalid(name, "Input should be a valid string")),
        }
    }

    /// Required string with `min_length` (in characters).
    pub fn str_req(&self, name: &str, min_len: usize) -> Result<String, OpError> {
        let v = self.present(name).ok_or_else(|| invalid(name, "Field required"))?;
        let s = Self::as_str(name, v)?;
        if s.chars().count() < min_len {
            return Err(invalid(name, &format!("String should have at least {min_len} character{}", if min_len == 1 { "" } else { "s" })));
        }
        Ok(s)
    }

    /// Optional string with a default (`null` is rejected like pydantic does for `str`).
    pub fn str_or(&self, name: &str, default: &str) -> Result<String, OpError> {
        match self.present(name) {
            None => Ok(default.to_string()),
            Some(v) => Self::as_str(name, v),
        }
    }

    /// `str | None = None`: missing or `null` -> `None`.
    pub fn str_opt(&self, name: &str) -> Result<Option<String>, OpError> {
        match self.present(name) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => Self::as_str(name, v).map(Some),
        }
    }

    // ---- bools ---------------------------------------------------------------------------

    /// pydantic v2 lax `bool`: JSON booleans, the numbers 0 / 1 (`0.0` / `1.0` too, `-0` included) and the strings
    /// `0 1 t f true false y n yes no on off` in any ASCII case with NO surrounding whitespace.
    fn as_bool(name: &str, v: &Value) -> Result<bool, OpError> {
        let err = || invalid(name, "Input should be a valid boolean, unable to interpret input");
        match v {
            Value::Bool(b) => Ok(*b),
            Value::Number(n) if number_is_int(n) => match py_int(&n.to_string()).map(|i| i.to_string()).as_deref() {
                Some("0") => Ok(false),
                Some("1") => Ok(true),
                _ => Err(err()),
            },
            Value::Number(n) => match n.to_string().parse::<f64>() {
                Ok(0.0) => Ok(false),
                Ok(1.0) => Ok(true),
                _ => Err(err()),
            },
            Value::String(s) => match s.to_ascii_lowercase().as_str() {
                "0" | "off" | "f" | "false" | "n" | "no" => Ok(false),
                "1" | "on" | "t" | "true" | "y" | "yes" => Ok(true),
                _ => Err(err()),
            },
            _ => Err(err()),
        }
    }

    pub fn bool_req(&self, name: &str) -> Result<bool, OpError> {
        let v = self.present(name).ok_or_else(|| invalid(name, "Field required"))?;
        Self::as_bool(name, v)
    }

    pub fn bool_or(&self, name: &str, default: bool) -> Result<bool, OpError> {
        match self.present(name) {
            None => Ok(default),
            Some(v) => Self::as_bool(name, v),
        }
    }

    /// `bool | None = None`.
    pub fn bool_opt(&self, name: &str) -> Result<Option<bool>, OpError> {
        match self.present(name) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => Self::as_bool(name, v).map(Some),
        }
    }

    // ---- ints ----------------------------------------------------------------------------

    /// pydantic v2 lax `int`, saturating at the `i128` range (callers range-check): JSON integers of any size, booleans,
    /// floats with a zero fractional part (`80.0`, `8e1`), and strings: surrounding whitespace (Unicode `White_Space`,
    /// which excludes U+001C..U+001F) is stripped, then `[+-]digits` with single underscores between digits, optionally
    /// followed by `.` and one or more `0` digits (`"80.0"`). Unicode digits, exponents and `inf`/`nan` are rejected.
    fn as_int(name: &str, v: &Value) -> Result<i128, OpError> {
        let err = || invalid(name, "Input should be a valid integer");
        match v {
            Value::Bool(b) => Ok(*b as i128),
            Value::Number(n) if number_is_int(n) => lax_int_str(&n.to_string()).ok_or_else(err),
            Value::Number(n) => match n.to_string().parse::<f64>() {
                Ok(f) if f.is_finite() && f.fract() == 0.0 => Ok(f as i128),
                _ => Err(err()),
            },
            Value::String(s) => lax_int_str(s).ok_or_else(err),
            _ => Err(err()),
        }
    }

    /// `int = Field(default, ge=min, le=max)`.
    pub fn int_range(&self, name: &str, default: i64, min: i64, max: i64) -> Result<i64, OpError> {
        let n = match self.present(name) {
            None => default as i128,
            Some(v) => Self::as_int(name, v)?,
        };
        if n < min as i128 {
            return Err(invalid(name, &format!("Input should be greater than or equal to {min}")));
        }
        if n > max as i128 {
            return Err(invalid(name, &format!("Input should be less than or equal to {max}")));
        }
        Ok(n as i64)
    }

    // ---- unions --------------------------------------------------------------------------

    /// Required `str | int | float | bool` (kept as its exact JSON type, like pydantic's smart union).
    pub fn config_value(&self, name: &str) -> Result<ConfigValue, OpError> {
        let v = self.present(name).ok_or_else(|| invalid(name, "Field required"))?;
        ConfigValue::from_json(v).ok_or_else(|| invalid(name, "Input should be a valid string, integer, number or boolean"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rules() {
        let v = json!({"s": "x", "e": "", "b": "maybe", "t": true, "n": 5, "extra": 1});
        let a = Args::new(&v).unwrap();
        assert_eq!(a.str_req("s", 1).unwrap(), "x");
        assert!(a.str_req("e", 1).is_err());
        assert!(a.str_req("missing", 1).is_err());
        assert!(a.bool_req("b").is_err());
        assert!(a.bool_req("t").unwrap());
        assert!(!a.bool_or("none", false).unwrap());
        assert!(a.int_range("n", 75, 75, 100).is_err());
        assert_eq!(a.int_range("absent", 75, 75, 100).unwrap(), 75);
        assert!(Args::new(&json!([1])).is_err());
        assert!(Args::new(&Value::Null).unwrap().bool_req("x").is_err());
    }

    /// Expectations GENERATED by running pydantic 2.14 (the legacy venv) with `tools/gen_pydantic_table.py`:
    /// `(raw JSON text, bool, int, str, str|int|float|bool)` where a cell is `ERR` or `<B|I|F|S>:<Python str() of the
    /// validated value>`. The int column uses `ge=-2**63, le=2**63-1` (a bigger int is a range error either way).
    const PYDANTIC_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    ("true", "B:True", "I:1", "ERR", "B:True"),
    ("false", "B:False", "I:0", "ERR", "B:False"),
    ("0", "B:False", "I:0", "ERR", "I:0"),
    ("1", "B:True", "I:1", "ERR", "I:1"),
    ("2", "ERR", "I:2", "ERR", "I:2"),
    ("-1", "ERR", "I:-1", "ERR", "I:-1"),
    ("0.0", "B:False", "I:0", "ERR", "F:0.0"),
    ("1.0", "B:True", "I:1", "ERR", "F:1.0"),
    ("0.5", "ERR", "ERR", "ERR", "F:0.5"),
    ("-0.0", "B:False", "I:0", "ERR", "F:-0.0"),
    ("1e0", "B:True", "I:1", "ERR", "F:1.0"),
    ("1e2", "ERR", "I:100", "ERR", "F:100.0"),
    ("\"\"", "ERR", "ERR", "S:", "S:"),
    ("\"0\"", "B:False", "I:0", "S:0", "S:0"),
    ("\"1\"", "B:True", "I:1", "S:1", "S:1"),
    ("\"t\"", "B:True", "ERR", "S:t", "S:t"),
    ("\"T\"", "B:True", "ERR", "S:T", "S:T"),
    ("\"F\"", "B:False", "ERR", "S:F", "S:F"),
    ("\"true\"", "B:True", "ERR", "S:true", "S:true"),
    ("\"TRUE\"", "B:True", "ERR", "S:TRUE", "S:TRUE"),
    ("\"True\"", "B:True", "ERR", "S:True", "S:True"),
    ("\"tRuE\"", "B:True", "ERR", "S:tRuE", "S:tRuE"),
    ("\"false\"", "B:False", "ERR", "S:false", "S:false"),
    ("\"y\"", "B:True", "ERR", "S:y", "S:y"),
    ("\"Y\"", "B:True", "ERR", "S:Y", "S:Y"),
    ("\"n\"", "B:False", "ERR", "S:n", "S:n"),
    ("\"no\"", "B:False", "ERR", "S:no", "S:no"),
    ("\"NO\"", "B:False", "ERR", "S:NO", "S:NO"),
    ("\"yes\"", "B:True", "ERR", "S:yes", "S:yes"),
    ("\"on\"", "B:True", "ERR", "S:on", "S:on"),
    ("\"ON\"", "B:True", "ERR", "S:ON", "S:ON"),
    ("\"off\"", "B:False", "ERR", "S:off", "S:off"),
    ("\"OFF\"", "B:False", "ERR", "S:OFF", "S:OFF"),
    ("\"maybe\"", "ERR", "ERR", "S:maybe", "S:maybe"),
    ("\" true\"", "ERR", "ERR", "S: true", "S: true"),
    ("\"true \"", "ERR", "ERR", "S:true ", "S:true "),
    ("\" 1\"", "ERR", "I:1", "S: 1", "S: 1"),
    ("\"1 \"", "ERR", "I:1", "S:1 ", "S:1 "),
    ("\"\\ttrue\"", "ERR", "ERR", "S:\u{9}true", "S:\u{9}true"),
    ("\"\\ntrue\"", "ERR", "ERR", "S:\u{a}true", "S:\u{a}true"),
    ("\"\\u00a0true\"", "ERR", "ERR", "S:\u{a0}true", "S:\u{a0}true"),
    ("\"1_0\"", "ERR", "I:10", "S:1_0", "S:1_0"),
    ("\"01\"", "ERR", "I:1", "S:01", "S:01"),
    ("\"00\"", "ERR", "I:0", "S:00", "S:00"),
    ("\"+1\"", "ERR", "I:1", "S:+1", "S:+1"),
    ("\"-0\"", "ERR", "I:0", "S:-0", "S:-0"),
    ("\"\\uff11\"", "ERR", "ERR", "S:\u{ff11}", "S:\u{ff11}"),
    ("\"tru\"", "ERR", "ERR", "S:tru", "S:tru"),
    ("null", "ERR", "ERR", "ERR", "ERR"),
    ("[]", "ERR", "ERR", "ERR", "ERR"),
    ("{}", "ERR", "ERR", "ERR", "ERR"),
    ("[1]", "ERR", "ERR", "ERR", "ERR"),
    ("\"80\"", "ERR", "I:80", "S:80", "S:80"),
    ("\"80.0\"", "ERR", "I:80", "S:80.0", "S:80.0"),
    ("\" 80 \"", "ERR", "I:80", "S: 80 ", "S: 80 "),
    ("\"8_0\"", "ERR", "I:80", "S:8_0", "S:8_0"),
    ("\"+80\"", "ERR", "I:80", "S:+80", "S:+80"),
    ("\"-80\"", "ERR", "I:-80", "S:-80", "S:-80"),
    ("\"80.5\"", "ERR", "ERR", "S:80.5", "S:80.5"),
    ("\"1e2\"", "ERR", "ERR", "S:1e2", "S:1e2"),
    ("\"0x50\"", "ERR", "ERR", "S:0x50", "S:0x50"),
    ("\"80\\n\"", "ERR", "I:80", "S:80\u{a}", "S:80\u{a}"),
    ("\"\\u0668\\u0660\"", "ERR", "ERR", "S:\u{668}\u{660}", "S:\u{668}\u{660}"),
    ("80.0", "ERR", "I:80", "ERR", "F:80.0"),
    ("8e1", "ERR", "I:80", "ERR", "F:80.0"),
    ("80.5", "ERR", "ERR", "ERR", "F:80.5"),
    ("9223372036854775807", "ERR", "I:9223372036854775807", "ERR", "I:9223372036854775807"),
    ("9223372036854775808", "ERR", "ERR", "ERR", "I:9223372036854775808"),
    ("-9223372036854775809", "ERR", "ERR", "ERR", "I:-9223372036854775809"),
    ("1e19", "ERR", "ERR", "ERR", "F:1e+19"),
    ("1e400", "ERR", "ERR", "ERR", "F:inf"),
    ("-1e400", "ERR", "ERR", "ERR", "F:-inf"),
    ("\"9223372036854775808\"", "ERR", "ERR", "S:9223372036854775808", "S:9223372036854775808"),
    ("\"99999999999999999999999999999999999999999\"", "ERR", "ERR", "S:99999999999999999999999999999999999999999", "S:99999999999999999999999999999999999999999"),
    ("\"inf\"", "ERR", "ERR", "S:inf", "S:inf"),
    ("\"nan\"", "ERR", "ERR", "S:nan", "S:nan"),
    ("\"1e3\"", "ERR", "ERR", "S:1e3", "S:1e3"),
    ("\"1_000\"", "ERR", "I:1000", "S:1_000", "S:1_000"),
    ("\"1__0\"", "ERR", "ERR", "S:1__0", "S:1__0"),
    ("\"_1\"", "ERR", "ERR", "S:_1", "S:_1"),
    ("\"1_\"", "ERR", "ERR", "S:1_", "S:1_"),
    ("\"\\u2003 7 \\u2003\"", "ERR", "I:7", "S:\u{2003} 7 \u{2003}", "S:\u{2003} 7 \u{2003}"),
    ("\"7\\u000b\"", "ERR", "I:7", "S:7\u{b}", "S:7\u{b}"),
    ("\"7\\u001c\"", "ERR", "ERR", "S:7\u{1c}", "S:7\u{1c}"),
    ("-0", "B:False", "I:0", "ERR", "I:0"),
    ("100000000000000000000", "ERR", "ERR", "ERR", "I:100000000000000000000"),
    ("\"1.0e2\"", "ERR", "ERR", "S:1.0e2", "S:1.0e2"),
    ("\".5e1\"", "ERR", "ERR", "S:.5e1", "S:.5e1"),
    ("\"5.\"", "ERR", "ERR", "S:5.", "S:5."),
    ("\"\\uff15\"", "ERR", "ERR", "S:\u{ff15}", "S:\u{ff15}"),
    ("12345678901234567890123", "ERR", "ERR", "ERR", "I:12345678901234567890123"),
    ("123456789.12345679", "ERR", "ERR", "ERR", "F:123456789.12345679"),
    ("1.5e300", "ERR", "ERR", "ERR", "F:1.5e+300"),
    ("5e-324", "ERR", "ERR", "ERR", "F:5e-324"),
    ("-5", "ERR", "I:-5", "ERR", "I:-5"),
    ("\"5.0\"", "ERR", "I:5", "S:5.0", "S:5.0"),
    ("\"5.00\"", "ERR", "I:5", "S:5.00", "S:5.00"),
    ("\"+5.0\"", "ERR", "I:5", "S:+5.0", "S:+5.0"),
    ("\"-5.0\"", "ERR", "I:-5", "S:-5.0", "S:-5.0"),
    ("\".0\"", "ERR", "ERR", "S:.0", "S:.0"),
    ("\"0.\"", "ERR", "ERR", "S:0.", "S:0."),
    ("\"1_0.0\"", "ERR", "I:10", "S:1_0.0", "S:1_0.0"),
    ("\"1.0_0\"", "ERR", "ERR", "S:1.0_0", "S:1.0_0"),
    ("\"5.0 \"", "ERR", "I:5", "S:5.0 ", "S:5.0 "),
    ("\"1.0\"", "ERR", "I:1", "S:1.0", "S:1.0"),
    ("\"0.0\"", "ERR", "I:0", "S:0.0", "S:0.0"),
    ("\"-0.0\"", "ERR", "I:0", "S:-0.0", "S:-0.0"),
    ("\"1.5\"", "ERR", "ERR", "S:1.5", "S:1.5"),
    ("\"1.0e0\"", "ERR", "ERR", "S:1.0e0", "S:1.0e0"),
    ("\"\\u00a05\"", "ERR", "I:5", "S:\u{a0}5", "S:\u{a0}5"),
    ("\"5\\u0085\"", "ERR", "I:5", "S:5\u{85}", "S:5\u{85}"),
    ("\"5\\ufeff\"", "ERR", "ERR", "S:5\u{feff}", "S:5\u{feff}"),
    ("\"\u{661}\"", "ERR", "ERR", "S:\u{661}", "S:\u{661}"),
    ("\"--5\"", "ERR", "ERR", "S:--5", "S:--5"),
    ("\"+-5\"", "ERR", "ERR", "S:+-5", "S:+-5"),
    ("\"5-\"", "ERR", "ERR", "S:5-", "S:5-"),
    ("\" \"", "ERR", "ERR", "S: ", "S: "),
    ("\"+\"", "ERR", "ERR", "S:+", "S:+"),
    ("\"-\"", "ERR", "ERR", "S:-", "S:-"),
    ("\"_\"", "ERR", "ERR", "S:_", "S:_"),
    ("\"0_0\"", "ERR", "I:0", "S:0_0", "S:0_0"),
    ("\"0_1\"", "ERR", "I:1", "S:0_1", "S:0_1"),
    ("\"00_1\"", "ERR", "I:1", "S:00_1", "S:00_1"),
    ];

    fn cell<T: std::fmt::Display>(r: Result<T, OpError>, tag: &str) -> String {
        match r {
            Ok(v) => format!("{tag}:{v}"),
            Err(_) => "ERR".into(),
        }
    }

    #[test]
    fn pydantic_lax_table() {
        for (raw, b, i, s, u) in PYDANTIC_TABLE {
            let value: Value = serde_json::from_str(raw).unwrap();
            let doc = json!({ "x": value });
            let a = Args::new(&doc).unwrap();
            let got_b = a.bool_req("x").map(|v| if v { "True" } else { "False" });
            assert_eq!(cell(got_b, "B"), *b, "bool of {raw}");
            assert_eq!(cell(a.int_range("x", 0, i64::MIN, i64::MAX), "I"), *i, "int of {raw}");
            assert_eq!(cell(a.str_req("x", 0), "S"), *s, "str of {raw}");
            let got_u = a.config_value("x").map(|c| match c {
                ConfigValue::Bool(_) => format!("B:{}", c.py_str()),
                ConfigValue::Int(_) | ConfigValue::BigInt(_) => format!("I:{}", c.py_str()),
                ConfigValue::Float(_) => format!("F:{}", c.py_str()),
                ConfigValue::Str(_) => format!("S:{}", c.py_str()),
            });
            assert_eq!(got_u.unwrap_or_else(|_| "ERR".into()), *u, "union of {raw}");
        }
    }

    #[test]
    fn int_range_applies_the_bounds_after_lax_parsing() {
        let v = json!({"a": "80.0", "b": " 8_0 ", "c": 80.0, "d": true, "e": "9223372036854775808", "f": "101"});
        let a = Args::new(&v).unwrap();
        assert_eq!(a.int_range("a", 75, 75, 100).unwrap(), 80);
        assert_eq!(a.int_range("b", 75, 75, 100).unwrap(), 80);
        assert_eq!(a.int_range("c", 75, 75, 100).unwrap(), 80);
        assert_eq!(a.int_range("d", 0, 0, 1).unwrap(), 1);
        assert!(a.int_range("e", 0, 0, 1000).is_err());
        assert!(a.int_range("f", 75, 75, 100).is_err());
        assert!(a.bool_or("a", false).is_err());
    }
}
