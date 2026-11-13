use org_docs::*;
fn front(yaml: &str) -> Result<serde_json::Value, FrontMatterError> {
    parse(&format!("---\n{yaml}---\nbody\n")).map(|doc| serde_json::Value::Object(doc.front_matter.unwrap()))
}
fn refused(yaml: &str) -> (String, Reason) {
    let error = front(yaml).expect_err(yaml);
    (error.field, error.reason)
}
#[test]
fn show() {
    for t in ["---\na: 1\n---\r\r\nbody", "---\na: 1\n---\r", "---\na: 1\n---\r\r"] {
        eprintln!("{t:?} => {:?}", parse(t).map(|d| (d.front_matter, d.body)));
    }
    for y in ["\"\": {a: &x 1}\n", "a: {\"\": &x 1}\n", "\"a.b\": {c: &x 1}\n", "a: {\"b.c\": &x 1}\n", "\"x[0]\": &a 1\n", "x: [&a 1]\n", "\"a\\nb\": &x 1\n",
              "v: -.nan\n", "v: +.nan\n", "v: -.NaN\n", "v: +.inf\n", "v: +.nan\n", "v: .nan\n", "\"<<\": 1\n", "'<<': 1\n", "a: {\"<<\": 1}\n", "\"<<\": &x 1\n"] {
        eprintln!("{y:?} => {:?}", front(y).map_err(|e| (e.field, e.reason)));
    }
    let _ = refused("a: 1\n");
}
