# POCs

Proofs of concept for ideas we may bring into Agentic Org. Each POC lives in its own
folder, is self-contained (own `package.json` / build), and is **not** wired into the
product apps or crates.

Every POC README states:

- **Question** — what we are trying to find out.
- **What it does** — the smallest thing built to answer it.
- **Outcome** — what we learned and what to carry into the product.

| POC | Question | Status |
| --- | --- | --- |
| [dss-workflow](dss-workflow/) | Can the DSS process (REQ-06) run as a Markdown + JSON workflow (REQ-01) with status-driven agent handoff (REQ-02), visualised with Svelte Flow? | Working demo |
