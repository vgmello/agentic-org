# Mock-up: a project that follows the DSS development process

Four screens drawn on 2026-11-18 to show the owner how a project looks when it follows
the desired state specification (DSS) development process. They are a concept, not
product code, and they were never rendered and checked for layout.

| File | Screen |
| --- | --- |
| `Main.dc.html` | The project's Overview: its development process and stages, the tabs the process adds, the seed team and the documentation sections |
| `Chat.dc.html` | Specifying with the product lead: question cards, and the required confirmation before a version is issued |
| `Docs.dc.html` | The Docs tab: the project's documentation by section, with versions, unissued changes and progress |
| `Document.dc.html` | One Product DSS document: issued and working text, the tasks that realize a version, code links and versions |
| `canvas.json` | Where the four artboards sit on the design canvas |

The files are artboards of a design canvas: each loads `./support.js` from the canvas
tool, so they do not render when opened directly from this folder. The canvas they
were published to is private to the owner.

What they illustrate is specified in
[development processes](../../../docs/design/development-processes.md) (the tabs a
process declares, the seed roster),
[desired state specifications](../../../docs/design/desired-state-specifications.md)
(issuing, convergence) and
[document 18](../../../docs/design/organization-data/18-development-processes-and-project-documentation.md).
Where a screen and those documents differ, the documents win. The names and numbers
are sample data. No Tailwind Plus component is used.
