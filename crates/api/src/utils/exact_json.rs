//! Free-form JSON for JavaScript readers: every integer as an exact string.
//!
//! A browser parses every JSON number as an IEEE double, so an integer above
//! 2^53 — 90,071,992 CKB in shannons, or a `since`-encoded epoch with its flag
//! bits set — is rounded before any frontend code sees it. Typed response
//! fields already carry amounts as strings; the free-form JSON the API passes
//! through (protocol-action metadata, decoded lock args) leaves through
//! [`integers_as_strings`], so every integer in it is a string holding its
//! exact decimal value, whatever its size: a field's JSON type never depends
//! on its value. Booleans, strings and null keep their type; arrays and
//! objects are converted element by element.

use serde_json::{Map, Value};

/// Convert every integer in `value` to its exact decimal string.
///
/// The JSON this serves is decoded from chain data, which has no fractions;
/// a non-integer number is a producer bug, reported with `context` and the
/// JSON path to it rather than passed on as a lossy double.
pub fn integers_as_strings(value: Value, context: &str) -> Result<Value, String> {
    convert(value, context, &mut String::from("$"))
}

fn convert(value: Value, context: &str, path: &mut String) -> Result<Value, String> {
    match value {
        Value::Number(number) => {
            if let Some(unsigned) = number.as_u64() {
                Ok(Value::String(unsigned.to_string()))
            } else if let Some(signed) = number.as_i64() {
                Ok(Value::String(signed.to_string()))
            } else {
                Err(format!(
                    "non-integer number {number} at {path} in {context}: chain-derived JSON carries integers only"
                ))
            }
        }
        Value::Array(items) => {
            let mut converted = Vec::with_capacity(items.len());
            for (index, item) in items.into_iter().enumerate() {
                let len = path.len();
                path.push_str(&format!("[{index}]"));
                converted.push(convert(item, context, path)?);
                path.truncate(len);
            }
            Ok(Value::Array(converted))
        }
        Value::Object(fields) => {
            let mut converted = Map::with_capacity(fields.len());
            for (key, item) in fields {
                let len = path.len();
                path.push('.');
                path.push_str(&key);
                let item = convert(item, context, path)?;
                path.truncate(len);
                converted.insert(key, item);
            }
            Ok(Value::Object(converted))
        }
        other => Ok(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn integers_of_any_size_become_exact_decimal_strings() {
        let value = json!({
            "small": 2,
            "aboveDoubles": 9_007_199_254_740_993_u64,
            "u64Max": u64::MAX,
            "negative": -9_007_199_254_740_995_i64,
            "nested": [{ "depositBlock": 19_695_550 }, [1, 2]],
            "flag": true,
            "text": "24",
            "none": null,
        });
        assert_eq!(
            integers_as_strings(value, "test").unwrap(),
            json!({
                "small": "2",
                "aboveDoubles": "9007199254740993",
                "u64Max": "18446744073709551615",
                "negative": "-9007199254740995",
                "nested": [{ "depositBlock": "19695550" }, ["1", "2"]],
                "flag": true,
                "text": "24",
                "none": null,
            })
        );
    }

    #[test]
    fn a_non_integer_number_is_an_error_naming_its_path() {
        let err =
            integers_as_strings(json!({ "a": [{ "rate": 0.5 }] }), "fiber metadata").unwrap_err();
        assert!(
            err.contains("$.a[0].rate") && err.contains("fiber metadata"),
            "{err}"
        );
    }
}
