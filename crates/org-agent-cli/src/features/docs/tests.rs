//! Unit tests for `agent-os doc index|ls|show|new|edit|move|delete|put`.

use serde_json::{json, Value};

use crate::runtime::transport::{OfficeTransport, Refusal, TransportOffice};

struct MockDocsTransport {
    calls: Vec<(String, Value)>,
}

impl MockDocsTransport {
    fn new() -> Self {
        Self { calls: Vec::new() }
    }
}

impl OfficeTransport for MockDocsTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.calls.push((method.to_owned(), params));
        Ok(match method {
            "docs.index" => json!({
                "project_id": "0193a5c0-0000-7000-8000-000000000010",
                "index_path": "state/projects/0193a5c0-0000-7000-8000-000000000010/docs/index.yaml",
                "sections": [{"path": "guides", "document_count": 1}],
                "documents": [],
                "media": []
            }),
            "docs.list" => json!({
                "documents": [{
                    "id": "0193a5c0-0000-7000-8000-000000000600",
                    "project_id": "0193a5c0-0000-7000-8000-000000000010",
                    "path": "guides/setup.md",
                    "section": "guides",
                    "kind": "guide",
                    "title": "Setup",
                    "status": "draft",
                    "revision": 1
                }],
                "next_cursor": null
            }),
            "docs.inspect" => json!({
                "document": {
                    "id": "0193a5c0-0000-7000-8000-000000000600",
                    "project_id": "0193a5c0-0000-7000-8000-000000000010",
                    "path": "guides/setup.md",
                    "section": "guides",
                    "kind": "guide",
                    "title": "Setup",
                    "status": "draft",
                    "revision": 1
                },
                "body_markdown": "# Setup\n"
            }),
            "docs.create" | "docs.update" | "docs.move" => json!({
                "document": {
                    "id": "0193a5c0-0000-7000-8000-000000000600",
                    "project_id": "0193a5c0-0000-7000-8000-000000000010",
                    "path": "guides/setup.md",
                    "section": "guides",
                    "kind": "guide",
                    "title": "Setup",
                    "status": "draft",
                    "revision": 2
                }
            }),
            "docs.delete" => json!({
                "deleted_id": "0193a5c0-0000-7000-8000-000000000600",
                "deleted_path": "guides/setup.md",
                "index_path": "state/projects/0193a5c0-0000-7000-8000-000000000010/docs/index.yaml"
            }),
            other => panic!("unexpected RPC `{other}`"),
        })
    }

    fn caller_member_id(&self) -> Option<String> {
        Some("0193a5c0-0000-7000-8000-000000000020".to_owned())
    }
}

fn run_doc(args: &[&str]) -> (org_command::Output, Vec<(String, Value)>) {
    let mut office = TransportOffice(MockDocsTransport::new());
    let owned: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
    let out = crate::execute_with_office("agent-os", &owned, &mut office);
    (out, office.0.calls)
}

#[test]
fn doc_reads_and_writes_dispatch_through_typed_rpcs() {
    let (out, calls) = run_doc(&["doc", "index", "--project", "core"]);
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(calls[0].0, "docs.index");

    let (out, calls) = run_doc(&["doc", "ls", "--project", "core", "--section", "guides"]);
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(calls[0].0, "docs.list");

    let (out, calls) = run_doc(&[
        "doc",
        "show",
        "--project",
        "core",
        "--document",
        "guides/setup.md",
    ]);
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(calls[0].0, "docs.inspect");

    let (out, calls) = run_doc(&[
        "doc",
        "new",
        "--project",
        "core",
        "--kind",
        "guide",
        "--title",
        "Setup",
        "--body",
        "# Hi",
    ]);
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(calls[0].0, "docs.create");

    // Path reference resolves via docs.inspect then calls docs.update / docs.move / docs.delete.
    let (out, calls) = run_doc(&[
        "doc",
        "edit",
        "--project",
        "core",
        "--document",
        "guides/setup.md",
        "--title",
        "Updated",
        "--if-match",
        "1",
    ]);
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(calls[0].0, "docs.inspect");
    assert_eq!(calls[1].0, "docs.update");

    let (out, calls) = run_doc(&[
        "doc",
        "move",
        "--project",
        "core",
        "--document",
        "guides/setup.md",
        "--new-section",
        "runbooks",
        "--if-match",
        "2",
    ]);
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(calls[1].0, "docs.move");

    let (out, calls) = run_doc(&[
        "doc",
        "delete",
        "--project",
        "core",
        "--document",
        "guides/setup.md",
        "--if-match",
        "2",
    ]);
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(calls[1].0, "docs.delete");
}

#[test]
fn doc_writes_reject_dry_run_without_sending_rpcs() {
    let (out, calls) = run_doc(&[
        "doc",
        "new",
        "--project",
        "core",
        "--kind",
        "guide",
        "--title",
        "Setup",
        "--dry-run",
    ]);
    assert_ne!(out.exit_code, 0);
    assert!(calls.is_empty(), "dry-run must send zero RPCs: {calls:?}");
}
