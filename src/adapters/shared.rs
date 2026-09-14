#![allow(dead_code)]

use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

static ENV_VAR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$\{([^}]+)\}").unwrap());

/// Recursively transforms `${VAR}` patterns in JSON Values using the given replacer.
pub fn transform_env_vars(value: &mut Value, replacer: &dyn Fn(&str) -> String) {
    match value {
        Value::String(s) => {
            let replaced = ENV_VAR_RE.replace_all(s, |caps: &regex::Captures| replacer(&caps[1]));
            *s = replaced.into_owned();
        }
        Value::Array(arr) => {
            for item in arr {
                transform_env_vars(item, replacer);
            }
        }
        Value::Object(obj) => {
            for (_, v) in obj.iter_mut() {
                transform_env_vars(v, replacer);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transform_string() {
        let mut val = Value::String("${VAR}".to_string());
        transform_env_vars(&mut val, &|name| format!("{{env:{}}}", name));
        assert_eq!(val, Value::String("{env:VAR}".to_string()));
    }

    #[test]
    fn test_transform_nested_object() {
        let mut val = serde_json::json!({
            "env": { "KEY": "${MY_VAR}" },
            "args": ["${ARG}"],
            "num": 42,
            "flag": true,
        });
        transform_env_vars(&mut val, &|name| format!("${{env:{}}}", name));
        assert_eq!(val["env"]["KEY"], "${env:MY_VAR}");
        assert_eq!(val["args"][0], "${env:ARG}");
        assert_eq!(val["num"], 42);
        assert_eq!(val["flag"], true);
    }
}
