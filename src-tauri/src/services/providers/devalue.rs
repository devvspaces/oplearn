//! SvelteKit `__data.json` payloads use the "devalue" format: a flat array where index 0 is the
//! root and objects/arrays hold indices into that array instead of values. `resolve` turns it
//! back into a plain JSON value.

use serde_json::{Map, Value};

use crate::error::{AppError, AppResult};

const MAX_DEPTH: usize = 64;

/// The last node with data in a SvelteKit `__data.json` reply, resolved.
pub fn page_data(reply: &Value) -> AppResult<Value> {
    let nodes = reply
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| changed("no nodes"))?;
    let data = nodes
        .iter()
        .rev()
        .find_map(|n| n.get("data").and_then(Value::as_array))
        .ok_or_else(|| changed("no data node"))?;
    resolve(data)
}

pub fn changed(what: &str) -> AppError {
    AppError::internal(format!(
        "NPTEL changed its page format ({what}); please report this"
    ))
}

pub fn resolve(flat: &[Value]) -> AppResult<Value> {
    if flat.is_empty() {
        return Ok(Value::Null);
    }
    hydrate(flat, 0, 0)
}

fn hydrate(flat: &[Value], index: i64, depth: usize) -> AppResult<Value> {
    if depth > MAX_DEPTH {
        return Err(changed("nesting too deep"));
    }
    // Negative indices are special values: undefined, hole, NaN, ±Infinity, -0.
    if index < 0 {
        return Ok(match index {
            -6 => Value::from(0),
            _ => Value::Null,
        });
    }
    let v = flat
        .get(index as usize)
        .ok_or_else(|| changed("dangling reference"))?;
    let at = |i: &Value| -> AppResult<Value> {
        let i = i.as_i64().ok_or_else(|| changed("bad reference"))?;
        hydrate(flat, i, depth + 1)
    };
    Ok(match v {
        Value::Array(items) => match items.first() {
            // Typed values: ["Date", iso], ["Set", …], ["Map", k, v, …], ["BigInt", s], …
            Some(Value::String(tag)) => match tag.as_str() {
                "Date" | "BigInt" => items.get(1).cloned().unwrap_or(Value::Null),
                "RegExp" => items.get(1).cloned().unwrap_or(Value::Null),
                "Object" => match items.get(1) {
                    Some(x) => x.clone(),
                    None => Value::Null,
                },
                "Set" => Value::Array(items[1..].iter().map(at).collect::<AppResult<_>>()?),
                "Map" | "null" => {
                    let mut m = Map::new();
                    for pair in items[1..].chunks(2) {
                        if let [k, v] = pair {
                            let key = match (tag.as_str(), at(k)?) {
                                ("null", _) => k.as_str().unwrap_or_default().to_string(),
                                (_, Value::String(s)) => s,
                                (_, other) => other.to_string(),
                            };
                            m.insert(key, at(v)?);
                        }
                    }
                    Value::Object(m)
                }
                _ => Value::Null,
            },
            _ => Value::Array(items.iter().map(at).collect::<AppResult<_>>()?),
        },
        Value::Object(o) => {
            let mut m = Map::new();
            for (k, i) in o {
                m.insert(k.clone(), at(i)?);
            }
            Value::Object(m)
        }
        other => other.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolves_references_nesting_and_special_values() {
        let flat = json!([
            {"courses": 1, "total": 6, "missing": -1, "when": 7},
            [2, 4],
            {"id": 3, "title": 5},
            101,
            {"id": 3, "title": 5},
            "Python",
            3484,
            ["Date", "2026-11-09T00:00:00Z"]
        ]);
        let v = resolve(flat.as_array().unwrap()).unwrap();
        assert_eq!(
            v,
            json!({
                "courses": [{"id": 101, "title": "Python"}, {"id": 101, "title": "Python"}],
                "total": 3484,
                "missing": null,
                "when": "2026-11-09T00:00:00Z"
            })
        );
    }

    #[test]
    fn page_data_takes_the_last_data_node_and_rejects_garbage() {
        let reply =
            json!({"type": "data", "nodes": [null, {"type": "data", "data": [{"a": 1}, "x"]}]});
        assert_eq!(page_data(&reply).unwrap(), json!({"a": "x"}));
        assert!(page_data(&json!({"nodes": [null]})).is_err());
        assert!(resolve(json!([{"a": 9}]).as_array().unwrap()).is_err());
        // A cycle is cut off instead of overflowing the stack.
        assert!(resolve(json!([{"self": 0}]).as_array().unwrap()).is_err());
    }
}
