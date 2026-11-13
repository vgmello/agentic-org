use org_docs::*;
fn front(yaml: &str) -> Result<serde_json::Value, FrontMatterError> {
    parse(&format!("---\n{yaml}---\nbody\n")).map(|d| serde_json::Value::Object(d.front_matter.unwrap()))
}
fn refused(yaml: &str) -> (String, Reason) { let e = front(yaml).unwrap_err(); (e.field, e.reason) }
#[test] fn f1_closing_fence() {
    for t in ["---\na: 1\n---\r\r\nbody", "---\na: 1\n---\r", "---\na: 1\n---\r\r"] { assert_eq!(parse(t).unwrap_err().reason, Reason::Unclosed, "{t:?}"); }
}
#[test] fn f2_quoted_merge() { assert_eq!(front("\"<<\": 1\n").unwrap(), serde_json::json!({"<<": 1})); }
#[test] fn f3_empty_key_path() { assert_ne!(refused("\"\": {a: &x 1}\n").0, refused("a: &x 1\n").0); }
#[test] fn f3b_dotted_key_path() { assert_ne!(refused("\"a.b\": {c: &x 1}\n").0, refused("a: {b: {c: &x 1}}\n").0); }
#[test] fn f4_signed_nan() { assert_eq!(front("v: -.nan\nw: +.NaN\n").unwrap(), serde_json::json!({"v": "-.nan", "w": "+.NaN"})); }
#[test] fn f5_deep_flow() { let y = format!("a: {}1{}\n", "[".repeat(300), "]".repeat(300)); assert_eq!(refused(&y).1, Reason::TooDeep); }
