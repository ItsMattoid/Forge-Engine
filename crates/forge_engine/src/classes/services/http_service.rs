//! HttpService: JSON, GUIDs and URL encoding. Requests to the internet are not supported (`GetAsync`,
//! `PostAsync` and `RequestAsync` say so): a Forge game runs offline.

use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Map, Number, Value as J};

use crate::core::*;

type R = Result<ScriptValue, String>;

/// How deep a table may nest when it is turned into JSON.
const MAX_DEPTH: usize = 64;

fn to_json(v: &ScriptValue, depth: usize) -> Result<J, String> {
    if depth > MAX_DEPTH {
        return Err("Cannot convert a table that is nested too deeply (or contains itself) to JSON".into());
    }
    Ok(match v {
        ScriptValue::Nil => J::Null,
        ScriptValue::Bool(b) => J::Bool(*b),
        ScriptValue::Num(n) => number(*n)?,
        ScriptValue::Str(s) | ScriptValue::Enum(s) => J::String(s.clone()),
        ScriptValue::List(items) => J::Array(items.iter().map(|i| to_json(i, depth + 1)).collect::<Result<_, _>>()?),
        // (An empty table is an empty array, as in Roblox.)
        ScriptValue::Map(entries) if entries.is_empty() => J::Array(Vec::new()),
        ScriptValue::Map(entries) => J::Object(entries.iter().map(|(k, v)| Ok((k.clone(), to_json(v, depth + 1)?))).collect::<Result<Map<_, _>, String>>()?),
        ScriptValue::Instance(_) => return Err("Can't convert an Instance to JSON".into()),
        ScriptValue::Vec2(_) | ScriptValue::Vec3(_) | ScriptValue::CFrame(..) | ScriptValue::Color(_) | ScriptValue::Data(..) | ScriptValue::EnumItem(..) => {
            return Err("Can't convert this value to JSON (only booleans, numbers, strings and tables of them)".into());
        }
        ScriptValue::Typed(_, inner) => to_json(inner, depth + 1)?,
        ScriptValue::Multi(_) => return Err("Can't convert several values to JSON".into()),
    })
}

fn number(n: f64) -> Result<J, String> {
    if !n.is_finite() {
        return Err("Can't convert a number that is NaN or infinity to JSON".into());
    }
    // Whole numbers are written as integers (`1`, not `1.0`).
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        return Ok(J::Number(Number::from(n as i64)));
    }
    Number::from_f64(n).map(J::Number).ok_or_else(|| "Can't convert this number to JSON".into())
}

fn from_json(j: &J) -> ScriptValue {
    match j {
        J::Null => ScriptValue::Nil,
        J::Bool(b) => ScriptValue::Bool(*b),
        J::Number(n) => ScriptValue::Num(n.as_f64().unwrap_or(0.0)),
        J::String(s) => ScriptValue::Str(s.clone()),
        J::Array(items) => ScriptValue::List(items.iter().map(from_json).collect()),
        J::Object(entries) => ScriptValue::Map(entries.iter().map(|(k, v)| (k.clone(), from_json(v))).collect()),
    }
}

fn json_encode(c: &mut MethodCall) -> R {
    let json = to_json(c.arg(0), 0)?;
    Ok(ScriptValue::Str(json.to_string()))
}

fn json_decode(c: &mut MethodCall) -> R {
    let text = c.string(0)?;
    serde_json::from_str::<J>(&text).map(|j| from_json(&j)).map_err(|e| format!("Can't parse JSON: {e}"))
}

/// The next 64 random bits (not for secrets: clock and counter run through SplitMix64).
fn entropy() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0x9E37_79B9_7F4A_7C15);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    let mut z = nanos ^ COUNTER.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A version 4 GUID, `{XXXXXXXX-XXXX-4XXX-YXXX-XXXXXXXXXXXX}` (braces unless the argument is false).
fn generate_guid(c: &mut MethodCall) -> R {
    let (a, b) = (entropy(), entropy());
    let (a, b) = (a & 0xFFFF_FFFF_FFFF_0FFF | 0x0000_0000_0000_4000, b & 0x3FFF_FFFF_FFFF_FFFF | 0x8000_0000_0000_0000);
    let text = format!("{:08X}-{:04X}-{:04X}-{:04X}-{:012X}", (a >> 32) as u32, (a >> 16) as u16, a as u16, (b >> 48) as u16, b & 0xFFFF_FFFF_FFFF);
    Ok(ScriptValue::Str(if c.opt_bool(0) == Some(false) { text } else { format!("{{{text}}}") }))
}

/// Percent-encodes everything but letters, digits and `-_.~`.
fn url_encode(c: &mut MethodCall) -> R {
    let text = c.string(0)?;
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    Ok(ScriptValue::Str(out))
}

static METHODS: &[MethodDesc] = &[
    MethodDesc::new("JSONEncode", json_encode).desc("Turns a table (or a boolean, number or string) into JSON text."),
    MethodDesc::new("JSONDecode", json_decode).desc("Turns JSON text into a table (or a boolean, number or string)."),
    MethodDesc::new("GenerateGUID", generate_guid).desc("A random identifier like {3F2504E0-4F89-41D3-9A0C-0305E82C3301}; GenerateGUID(false) has no braces."),
    MethodDesc::new("UrlEncode", url_encode).desc("Percent-encodes text for use in a URL."),
];

static PROPS: &[PropMeta] = &[PropMeta::boolean("HttpEnabled", false).group("HttpService").desc("Roblox's switch for requests to the internet. Forge cannot make requests whatever it says.")];

pub static CLASS: ClassDesc = ClassDesc::new("HttpService", Some("Instance"))
    .describe("JSON, GUIDs and URL encoding (no internet requests).")
    .service(18)
    .hidden_in_explorer()
    .groups(&["HttpService"])
    .props(PROPS)
    .methods(METHODS)
    .unsupported(&["GetAsync", "PostAsync", "RequestAsync", "RequestInternal", "CreateWebStreamClient"]);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_goes_both_ways() {
        let table = ScriptValue::Map(vec![("a".into(), ScriptValue::List(vec![ScriptValue::Num(1.0), ScriptValue::Num(2.5), ScriptValue::Bool(true)]))]);
        assert_eq!(to_json(&table, 0).unwrap().to_string(), r#"{"a":[1,2.5,true]}"#);
        assert_eq!(to_json(&ScriptValue::Map(Vec::new()), 0).unwrap().to_string(), "[]", "an empty table is an array");
        assert!(to_json(&ScriptValue::Num(f64::NAN), 0).is_err());
        assert!(to_json(&ScriptValue::Vec3(bevy::math::Vec3::ONE), 0).is_err());
        let back = from_json(&serde_json::from_str(r#"{"n":null,"s":"x","list":[1,null,3]}"#).unwrap());
        let ScriptValue::Map(entries) = back else { panic!("an object is a table") };
        assert_eq!(entries.len(), 3);
    }
}