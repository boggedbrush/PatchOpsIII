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

pub struct Args<'a> {
    obj: Option<&'a Map<String, Value>>,
}

fn invalid(field: &str, msg: &str) -> OpError {
    OpError::Validation(format!("{field}: {msg}"))
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

    fn as_bool(name: &str, v: &Value) -> Result<bool, OpError> {
        let err = || invalid(name, "Input should be a valid boolean, unable to interpret input");
        match v {
            Value::Bool(b) => Ok(*b),
            Value::Number(n) => match n.as_f64() {
                Some(0.0) => Ok(false),
                Some(1.0) => Ok(true),
                _ => Err(err()),
            },
            Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
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

    fn as_int(name: &str, v: &Value) -> Result<i64, OpError> {
        let err = || invalid(name, "Input should be a valid integer");
        match v {
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(i)
                } else {
                    match n.as_f64() {
                        Some(f) if f.fract() == 0.0 && f.abs() < 9.0e18 => Ok(f as i64),
                        _ => Err(err()),
                    }
                }
            }
            Value::Bool(b) => Ok(*b as i64),
            Value::String(s) => s.trim().parse::<i64>().map_err(|_| err()),
            _ => Err(err()),
        }
    }

    /// `int = Field(default, ge=min, le=max)`.
    pub fn int_range(&self, name: &str, default: i64, min: i64, max: i64) -> Result<i64, OpError> {
        let n = match self.present(name) {
            None => default,
            Some(v) => Self::as_int(name, v)?,
        };
        if n < min {
            return Err(invalid(name, &format!("Input should be greater than or equal to {min}")));
        }
        if n > max {
            return Err(invalid(name, &format!("Input should be less than or equal to {max}")));
        }
        Ok(n)
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
}
