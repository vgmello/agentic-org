# Architecture pages

Two self-contained HTML pages for people. Open them in a browser.

| Page | What it is for |
| --- | --- |
| [architecture-overview.html](architecture-overview.html) | The product in one read: the parts, the main rules, what runs today, and what the owner decided in the 2026-11-17 architecture review. |
| [system-architecture.html](system-architecture.html) | The detailed reference: every process and connection, the libraries, the data, the main flows, and each decision with what the code does today. |

These are snapshots written on 2026-11-17 from `v1-integration`. They explain; they do not decide. The contracts are the documents listed in the [documentation authority map](../design/documentation-authority.md), and where a page and a contract differ, the contract wins. The decisions from the review, with what was applied for each, are in the [conflict resolution plan](../design/conflict-resolution-plan.md).

The pages load two fonts from Google Fonts and fall back to system fonts offline. They are not regenerated automatically; update them when the architecture changes.
