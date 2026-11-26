/* Seeded Chat attachments, so the feature shows without uploading. Everything is generated in the page
   (canvas PNGs, a one-page PDF, text files) as object URLs; nothing is fetched. */
import { CH } from "../data.js";

const blobFile = (name, type, parts, alt) => {
  const blob = new Blob(parts, { type });
  return { id: `seed-${name}`, name, type, size: blob.size, url: URL.createObjectURL(blob), alt };
};
function pngBytes(canvas) {
  const bin = atob(canvas.toDataURL("image/png").split(",")[1]);
  return Uint8Array.from(bin, (c) => c.charCodeAt(0));
}

/* A small mock screenshot of the app's navigation: a sidebar of sections and a content area. */
function drawScreenshot(sections, accent) {
  const c = document.createElement("canvas");
  c.width = 640; c.height = 400;
  const g = c.getContext("2d");
  if (!g) return null;
  g.fillStyle = "#0f172a"; g.fillRect(0, 0, 640, 400);
  g.fillStyle = "#111c33"; g.fillRect(0, 0, 168, 400);
  g.font = "600 15px sans-serif"; g.fillStyle = "#e2e8f0"; g.fillText("Agentic Org", 20, 34);
  sections.forEach((s, i) => {
    const y = 64 + i * 38;
    if (i === 0) { g.fillStyle = accent; g.fillRect(12, y - 20, 144, 30); }
    g.fillStyle = i === 0 ? "#0b1220" : "#cbd5e1"; g.font = "500 14px sans-serif"; g.fillText(s, 24, y);
  });
  g.fillStyle = "#e2e8f0"; g.font = "600 22px sans-serif"; g.fillText(sections[0], 196, 46);
  for (let i = 0; i < 5; i++) {
    g.fillStyle = "#1e293b"; g.fillRect(196, 74 + i * 60, 412, 46);
    g.fillStyle = i < 2 ? accent : "#475569"; g.fillRect(208, 90 + i * 60, 14, 14);
    g.fillStyle = "#94a3b8"; g.fillRect(234, 90 + i * 60, 180 + (i % 3) * 50, 12);
  }
  return c;
}
function screenshot(name, sections, accent, alt) {
  const c = drawScreenshot(sections, accent);
  return c ? blobFile(name, "image/png", [pngBytes(c)], alt) : null;
}

/* A valid one-page PDF with a few lines of text, offsets computed so viewers open it. */
function pdfBytes(lines) {
  const text = lines.map((l, i) => `BT /F1 ${i ? 11 : 16} Tf 56 ${760 - i * 22} Td (${l.replace(/[()\\]/g, "\\$&")}) Tj ET`).join("\n");
  const objs = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    `<< /Length ${text.length} >>\nstream\n${text}\nendstream`,
  ];
  let out = "%PDF-1.4\n";
  const offsets = objs.map((o, i) => { const at = out.length; out += `${i + 1} 0 obj\n${o}\nendobj\n`; return at; });
  const xref = out.length;
  out += `xref\n0 ${objs.length + 1}\n0000000000 65535 f \n${offsets.map((o) => `${String(o).padStart(10, "0")} 00000 n \n`).join("")}`;
  return out + `trailer\n<< /Size ${objs.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
}

const LEASE_LOG = `2026-11-26T08:58:02Z office=hq custody.lease renew id=L-118 ttl=30s ok
2026-11-26T08:58:31Z office=hq shutdown requested
2026-11-26T08:58:31Z office=hq custody.lease renew id=L-118 ttl=30s RACE: renew after shutdown began
2026-11-26T08:58:32Z office=sat custody.lease acquire id=L-118 refused: still held by hq
2026-11-26T08:59:01Z office=sat custody.lease acquire id=L-118 ok (expired)
`;
const FRAME_DIFF = `--- a/crates/org-sync-proto/src/frame.rs
+++ b/crates/org-sync-proto/src/frame.rs
@@ -12,7 +12,8 @@ pub struct FrameHeader {
-    pub len: u16,
+    pub len: u32,
+    pub version: u8,
`;

function attach(chId, mid, files) {
  const m = CH[chId]?.msgs.find((x) => x.id === mid);
  if (m && !m.files) m.files = files.filter(Boolean);
}
function seedChatAttachments() {
  attach("agency-v1", "m1", [
    screenshot("ia-v2-nav-before.png", ["Home", "Inbox", "Projects", "Work", "Chat", "Insights"], "#38bdf8", "Screenshot: navigation before IA v2, six top-level sections"),
    screenshot("ia-v2-nav-after.png", ["Home", "Inbox", "Work", "Chat"], "#34d399", "Screenshot: navigation after IA v2, four top-level sections"),
  ]);
  attach("platform", "p1", [
    blobFile("custody-lock-ruling.pdf", "application/pdf", [pdfBytes(["Custody lock ruling", "Lease: safer; recovers when an office goes offline.", "Per-office lock: about a day faster to finish.", "Owner decides. Details in T-A118."])]),
    blobFile("lease-race.log", "text/plain", [LEASE_LOG]),
  ]);
  attach("a2a-kai-rowan", "k1", [blobFile("frame-header.diff", "text/x-diff", [FRAME_DIFF])]);
}
seedChatAttachments();
