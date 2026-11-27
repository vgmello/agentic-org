//! Comprehensive unit & security tests for `org-doc-render` (`TASK-98.7` AC #2 & AC #4).

use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use image::{ImageFormat, Rgba, RgbaImage};

use crate::{
    count_pdf_pages, extract_attachment_refs, BoundedDocumentRenderer, DocumentRenderer,
    PdfWriterRenderer, RenderError, RenderLimits, RenderRequest, RenderedPdf, ResolvedImage,
    SourceKind, MAX_SOURCE_BYTES,
};

fn tiny_png_bytes() -> Vec<u8> {
    let mut img = RgbaImage::new(4, 4);
    for px in img.pixels_mut() {
        *px = Rgba([20, 120, 220, 255]);
    }
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

fn assert_valid_pdf(pdf: &RenderedPdf, min_pages: u32) {
    assert!(pdf.bytes.starts_with(b"%PDF-"), "missing %PDF- header");
    assert_eq!(pdf.byte_count, pdf.bytes.len() as u64);
    assert!(
        pdf.page_count >= min_pages,
        "expected >= {min_pages} pages, got {}",
        pdf.page_count
    );
    assert_eq!(count_pdf_pages(&pdf.bytes).unwrap(), pdf.page_count);
}

#[test]
fn markdown_with_headings_tables_lists_and_code_renders_valid_pdf() {
    let renderer = BoundedDocumentRenderer::production();
    let md = "\
# Quarterly Engineering Report

This is a **bold**, *italic*, and `inline code` summary with a [reference](https://example.com).

## Metrics Table

| Area | Status | Notes |
| --- | --- | --- |
| Render Engine | Green | Pure Rust zero-Chrome |
| Security Air-Gap | Green | No FS or network fetches |

> Key takeaway: every document renders deterministically in memory.

- First bullet item
- Second bullet item

1. Step one
2. Step two

```rust
fn main() {
    println!(\"hello pdf\");
}
```
";
    let pdf = renderer
        .render_markdown_to_pdf(&RenderRequest::markdown(md).with_title("Q4 Report"))
        .expect("markdown renders cleanly");
    assert_valid_pdf(&pdf, 1);
}

#[test]
fn html_with_multipage_table_and_at_page_header_footer_renders_multiple_pages() {
    let renderer = BoundedDocumentRenderer::production();
    let mut rows = String::new();
    for i in 1..=90 {
        rows.push_str(&format!(
            "<tr><td>ROW-{i:03}</td><td>Automated delivery record {i}</td><td>Passed</td></tr>\n"
        ));
    }
    let html = format!(
        r#"<!doctype html>
<html>
<head><title>Audit Matrix</title></head>
<body>
  <h1>Release Verification Matrix</h1>
  <p>Multi-page table with repeating headers and running @page header/footer.</p>
  <table>
    <thead>
      <tr><th>ID</th><th>Description</th><th>Outcome</th></tr>
    </thead>
    <tbody>
      {rows}
    </tbody>
  </table>
</body>
</html>"#
    );
    let css = r#"
@page {
  size: A4;
  margin: 60pt 48pt;
  @top-left { content: "Agentic Org Audit"; font-size: 8pt; color: #555555; }
  @bottom-right { content: "Page " counter(page) " of " counter(pages); font-size: 8pt; }
}
h1 { color: #1a3a6c; }
"#;
    let pdf = renderer
        .render_html_to_pdf(&RenderRequest::html(html).with_css(css))
        .expect("html+css renders cleanly");
    assert_valid_pdf(&pdf, 2);
}

#[test]
fn inline_base64_png_and_preauthorized_attachment_images_render_cleanly() {
    let png = tiny_png_bytes();
    let data_uri = format!("data:image/png;base64,{}", STANDARD.encode(&png));
    let att_id = "0193a5c0-0000-7000-8000-000000000701";

    let html = format!(
        r#"<h1>Charts</h1>
<img src="{data_uri}" alt="Inline chart" width="64pt">
<img src="attachment:{att_id}" alt="Authorized attachment" width="64pt">"#
    );
    assert_eq!(
        extract_attachment_refs(SourceKind::Html, &html, None),
        vec![att_id.to_owned()]
    );

    let renderer = BoundedDocumentRenderer::production();
    // Without caller-authorized attachment bytes, attachment:<id> MUST fail closed!
    let denied = renderer
        .render_html_to_pdf(&RenderRequest::html(html.clone()))
        .unwrap_err();
    assert!(
        matches!(denied, RenderError::ForbiddenResourceUrl { .. }),
        "{denied:?}"
    );

    // With pre-authorized attachment bytes, it succeeds cleanly.
    let mut req = RenderRequest::html(html);
    req.attachments.insert(
        att_id.to_owned(),
        ResolvedImage {
            mime_type: "image/png".to_owned(),
            bytes: png,
        },
    );
    let pdf = renderer
        .render_html_to_pdf(&req)
        .expect("authorized images render cleanly");
    assert_valid_pdf(&pdf, 1);
}

#[test]
fn ssrf_local_exfil_active_scripts_and_svg_data_uris_all_fail_closed() {
    let renderer = BoundedDocumentRenderer::production();

    let bad_html_cases = [
        r#"<img src="file:///etc/passwd">"#,
        r#"<img src="/etc/passwd">"#,
        r#"<img src="https://evil.example/a.png">"#,
        r#"<img src="http://169.254.169.254/latest/meta-data/">"#,
        r#"<img src="//evil.example/a.png">"#,
        r#"<img src="../secret.png">"#,
        r#"<img src="data:image/svg+xml;base64,PHN2Zy8+">"#,
        r#"<img src="data:text/html;base64,PHNjcmlwdD4=">"#,
        r#"<script>alert(1)</script><p>hi</p>"#,
        r#"<iframe src="https://evil.example"></iframe>"#,
        r#"<object data="file:///etc/passwd"></object>"#,
        r#"<embed src="file:///etc/passwd">"#,
        r#"<svg><foreignObject><p>hi</p></foreignObject></svg>"#,
        r#"<meta http-equiv="refresh" content="0;url=https://evil.example"><p>hi</p>"#,
        r#"<p onclick="alert(1)">hi</p>"#,
        r#"<a href="javascript:alert(1)">click</a>"#,
        r#"<a href="file:///etc/passwd">passwd</a>"#,
        r#"<div style="background-image: url(file:///etc/shadow)">x</div>"#,
        r#"<style>@import url("file:///etc/passwd");</style><p>x</p>"#,
        r#"<style>body { background: \75 rl(https://evil.example/a.png); }</style><p>x</p>"#,
    ];

    for case in bad_html_cases {
        let err = renderer
            .render_html_to_pdf(&RenderRequest::html(case))
            .expect_err(&format!("expected HTML case to fail closed: {case}"));
        assert!(
            matches!(
                err,
                RenderError::ForbiddenResourceUrl { .. } | RenderError::ForbiddenContent { .. }
            ),
            "unexpected error for `{case}`: {err:?}"
        );
    }

    // External CSS vectors
    for bad_css in [
        "body { background-image: url(file:///etc/shadow); }",
        "body { background-image: url(https://evil.example/bg.png); }",
        "@import 'https://evil.example/a.css';",
        "@\\69 mport '/etc/passwd';",
        "body { width: expression(alert(1)); }",
    ] {
        let err = renderer
            .render_html_to_pdf(&RenderRequest::html("<p>safe</p>").with_css(bad_css))
            .expect_err(&format!("expected CSS case to fail closed: {bad_css}"));
        assert!(
            matches!(
                err,
                RenderError::ForbiddenResourceUrl { .. } | RenderError::ForbiddenContent { .. }
            ),
            "unexpected CSS error for `{bad_css}`: {err:?}"
        );
    }

    // Markdown SSRF / exfil / raw-HTML script vectors
    for bad_md in [
        "![exfil](file:///etc/passwd)",
        "![ssrf](https://evil.example/a.png)",
        "![svg](data:image/svg+xml;base64,PHN2Zy8+)",
        "[bad](javascript:alert(1))",
        "[local](file:///etc/passwd)",
        "hello\n\n<script>alert(1)</script>\n",
        "hello <img src=\"file:///etc/passwd\">",
    ] {
        let err = renderer
            .render_markdown_to_pdf(&RenderRequest::markdown(bad_md))
            .expect_err(&format!("expected Markdown case to fail closed: {bad_md}"));
        assert!(
            matches!(
                err,
                RenderError::ForbiddenResourceUrl { .. } | RenderError::ForbiddenContent { .. }
            ),
            "unexpected Markdown error for `{bad_md}`: {err:?}"
        );
    }
}

#[test]
fn oversized_input_above_2_mib_fails_closed_before_parsing() {
    let renderer = BoundedDocumentRenderer::production();
    let big_source = "a".repeat(MAX_SOURCE_BYTES + 1);
    let err = renderer
        .render_markdown_to_pdf(&RenderRequest::markdown(big_source))
        .unwrap_err();
    assert!(matches!(err, RenderError::InputTooLarge { .. }), "{err:?}");

    // Combined source + CSS over 2 MiB also fails closed.
    let half = "a".repeat(MAX_SOURCE_BYTES / 2 + 10);
    let err = renderer
        .render_html_to_pdf(&RenderRequest::html(half.clone()).with_css(half))
        .unwrap_err();
    assert!(matches!(err, RenderError::InputTooLarge { .. }), "{err:?}");
}

#[test]
fn exceeding_100_pages_fails_closed() {
    let renderer = BoundedDocumentRenderer::production();
    // 105 explicit page breaks -> 106 pages (> 100 cap).
    let mut html = String::from("<body>");
    for i in 0..105 {
        html.push_str(&format!(
            r#"<p style="page-break-before: always">Page {i}</p>"#
        ));
    }
    html.push_str("</body>");
    let err = renderer
        .render_html_to_pdf(&RenderRequest::html(html))
        .unwrap_err();
    assert!(
        matches!(err, RenderError::PageLimitExceeded { max: 100, .. }),
        "{err:?}"
    );
}

#[test]
fn concurrency_semaphore_enforces_max_two_permits_and_timeout_and_panic_release() {
    struct BlockingRenderer {
        active: Arc<AtomicUsize>,
        peak: Arc<AtomicUsize>,
        release: Arc<AtomicBool>,
        entered: Arc<Barrier>,
        should_panic: Arc<AtomicBool>,
        real: PdfWriterRenderer,
    }

    impl DocumentRenderer for BlockingRenderer {
        fn render_markdown_to_pdf(&self, req: &RenderRequest) -> Result<RenderedPdf, RenderError> {
            if self.should_panic.load(Ordering::SeqCst) {
                panic!("simulated renderer panic");
            }
            let cur = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(cur, Ordering::SeqCst);
            if req.source.contains("HOLD") {
                self.entered.wait();
                while !self.release.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            let res = self.real.render_markdown_to_pdf(req);
            self.active.fetch_sub(1, Ordering::SeqCst);
            res
        }

        fn render_html_to_pdf(&self, req: &RenderRequest) -> Result<RenderedPdf, RenderError> {
            self.render_markdown_to_pdf(req)
        }
    }

    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(Barrier::new(3));
    let should_panic = Arc::new(AtomicBool::new(false));

    let bounded = Arc::new(BoundedDocumentRenderer::new(
        BlockingRenderer {
            active: Arc::clone(&active),
            peak: Arc::clone(&peak),
            release: Arc::clone(&release),
            entered: Arc::clone(&entered),
            should_panic: Arc::clone(&should_panic),
            real: PdfWriterRenderer::default(),
        },
        RenderLimits {
            max_concurrency: 2,
            timeout: Duration::from_millis(120),
            ..RenderLimits::default()
        },
    ));

    let r1 = Arc::clone(&bounded);
    let t1 =
        std::thread::spawn(move || r1.render_markdown_to_pdf(&RenderRequest::markdown("HOLD 1")));
    let r2 = Arc::clone(&bounded);
    let t2 =
        std::thread::spawn(move || r2.render_markdown_to_pdf(&RenderRequest::markdown("HOLD 2")));

    // Wait until both permits are actively held.
    entered.wait();
    assert_eq!(active.load(Ordering::SeqCst), 2);

    // A 3rd concurrent render must fail closed with Busy instead of bypassing max=2!
    let third = bounded
        .render_markdown_to_pdf(&RenderRequest::markdown("FAST 3"))
        .unwrap_err();
    assert_eq!(third, RenderError::Busy);
    assert_eq!(peak.load(Ordering::SeqCst), 2);

    // Release the held workers (they will time out on their callers, then drop permits cleanly).
    release.store(true, Ordering::SeqCst);
    let _ = t1.join().unwrap();
    let _ = t2.join().unwrap();

    // Wait briefly for detached worker permits to drop, then confirm panics also release permits.
    for _ in 0..40 {
        if active.load(Ordering::SeqCst) == 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    should_panic.store(true, Ordering::SeqCst);
    let panicked = bounded
        .render_markdown_to_pdf(&RenderRequest::markdown("PANIC"))
        .unwrap_err();
    assert!(matches!(panicked, RenderError::RenderFailed(_)));

    // Permits must still be intact after a panic.
    should_panic.store(false, Ordering::SeqCst);
    let ok = bounded
        .render_markdown_to_pdf(&RenderRequest::markdown("# Recovered"))
        .expect("permits released after panic");
    assert_valid_pdf(&ok, 1);
}

#[test]
fn slow_renderer_enforces_timeout_fail_closed() {
    struct SlowRenderer;
    impl DocumentRenderer for SlowRenderer {
        fn render_markdown_to_pdf(&self, _: &RenderRequest) -> Result<RenderedPdf, RenderError> {
            std::thread::sleep(Duration::from_millis(250));
            PdfWriterRenderer::default().render_markdown_to_pdf(&RenderRequest::markdown("# Late"))
        }
        fn render_html_to_pdf(&self, req: &RenderRequest) -> Result<RenderedPdf, RenderError> {
            self.render_markdown_to_pdf(req)
        }
    }
    let bounded = BoundedDocumentRenderer::new(
        SlowRenderer,
        RenderLimits {
            timeout: Duration::from_millis(40),
            ..RenderLimits::default()
        },
    );
    let err = bounded
        .render_markdown_to_pdf(&RenderRequest::markdown("# Slow"))
        .unwrap_err();
    assert!(matches!(err, RenderError::Timeout { .. }), "{err:?}");
}

#[test]
fn invalid_pdf_output_from_backend_fails_closed() {
    struct CorruptRenderer;
    impl DocumentRenderer for CorruptRenderer {
        fn render_markdown_to_pdf(&self, _: &RenderRequest) -> Result<RenderedPdf, RenderError> {
            Ok(RenderedPdf {
                bytes: b"NOT A PDF".to_vec(),
                page_count: 1,
                byte_count: 9,
            })
        }
        fn render_html_to_pdf(&self, req: &RenderRequest) -> Result<RenderedPdf, RenderError> {
            self.render_markdown_to_pdf(req)
        }
    }
    let bounded = BoundedDocumentRenderer::new(CorruptRenderer, RenderLimits::default());
    let err = bounded
        .render_markdown_to_pdf(&RenderRequest::markdown("# Test"))
        .unwrap_err();
    assert!(matches!(err, RenderError::InvalidPdf(_)), "{err:?}");
}

#[test]
fn debug_implementations_redact_payload_bytes() {
    let req = RenderRequest::markdown("SUPER_SECRET_BODY").with_css("SECRET_CSS");
    let dbg = format!("{req:?}");
    assert!(!dbg.contains("SUPER_SECRET_BODY"), "{dbg}");
    assert!(!dbg.contains("SECRET_CSS"), "{dbg}");
    let img = ResolvedImage {
        mime_type: "image/png".into(),
        bytes: b"SECRET_IMAGE_BYTES".to_vec(),
    };
    assert!(!format!("{img:?}").contains("SECRET_IMAGE_BYTES"));
    let mut map = BTreeMap::new();
    map.insert("a".to_owned(), img);
    assert_eq!(map.len(), 1);
}

fn solid_png_bytes(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    let img = RgbaImage::from_pixel(width, height, Rgba(rgba));
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

#[test]
fn multibyte_utf8_in_css_never_panics_and_renders_valid_pdf() {
    let renderer = BoundedDocumentRenderer::production();
    let css = r#"
@page {
  @top-left { content: —; }
  @top-right { content: "Café — €100 " counter(page) "/" counter(pages); }
}
h1 { color: #€; background: #€€; }
.über-klasse { color: #123456; }
p { color: #€a; }
"#;
    let pdf = renderer
        .render_html_to_pdf(
            &RenderRequest::html(r#"<h1 class="über-klasse">Über</h1><p>Résumé €100 — ok</p>"#)
                .with_css(css),
        )
        .expect("multibyte UTF-8 CSS must not panic");
    assert_valid_pdf(&pdf, 1);
}

#[test]
fn image_decompression_bomb_deduplication_and_markdown_raw_img_limit() {
    let renderer = BoundedDocumentRenderer::production();
    let att_id = "0193a5c0-0000-7000-8000-000000000777";

    // (a) Small compressed high-dimension PNG bomb (2048x2048 -> 16 MiB RGBA > 4 MiB per-image cap).
    let bomb = solid_png_bytes(2048, 2048, [10, 20, 30, 255]);
    assert!(bomb.len() < 100_000, "solid PNG compresses tiny");
    let mut bomb_req = RenderRequest::html(format!(r#"<img src="attachment:{att_id}">"#));
    bomb_req.attachments.insert(
        att_id.to_owned(),
        ResolvedImage {
            mime_type: "image/png".to_owned(),
            bytes: bomb,
        },
    );
    let err = renderer.render_html_to_pdf(&bomb_req).unwrap_err();
    assert!(
        matches!(err, RenderError::ForbiddenContent { .. }),
        "{err:?}"
    );

    // (b) 32x repeated 512x512 RGBA image (1 MiB each): deduplicates cleanly within 16 MiB cap!
    let tile = solid_png_bytes(512, 512, [40, 80, 120, 255]);
    let repeated_html: String = (0..32)
        .map(|_| format!(r#"<img src="attachment:{att_id}" width="16pt">"#))
        .collect();
    let mut dedup_req = RenderRequest::html(repeated_html);
    dedup_req.attachments.insert(
        att_id.to_owned(),
        ResolvedImage {
            mime_type: "image/png".to_owned(),
            bytes: tile,
        },
    );
    let dedup_pdf = renderer
        .render_html_to_pdf(&dedup_req)
        .expect("32x repeated image deduplicates cleanly");
    assert_valid_pdf(&dedup_pdf, 1);

    // (b2) 5 distinct 1024x1024 RGBA images (5 * 4 MiB = 20 MiB > 16 MiB cumulative cap) fail closed.
    let mut over_budget_html = String::new();
    let mut over_req = RenderRequest::html(String::new());
    for i in 1..=5u8 {
        let id = format!("0193a5c0-0000-7000-8000-00000000078{i}");
        over_budget_html.push_str(&format!(r#"<img src="attachment:{id}" width="16pt">"#));
        over_req.attachments.insert(
            id,
            ResolvedImage {
                mime_type: "image/png".to_owned(),
                bytes: solid_png_bytes(1024, 1024, [i * 20, 40, 60, 255]),
            },
        );
    }
    over_req.source = over_budget_html;
    let err = renderer.render_html_to_pdf(&over_req).unwrap_err();
    assert!(
        matches!(err, RenderError::ForbiddenContent { .. }),
        "{err:?}"
    );

    // (c) 40 raw `<img src="attachment:...">` tags in Markdown share MAX_EMBEDDED_IMAGES=32 cap.
    let tiny = tiny_png_bytes();
    let forty_raw_md: String = (0..40)
        .map(|_| format!("<img src=\"attachment:{att_id}\">\n"))
        .collect();
    let mut md_req = RenderRequest::markdown(forty_raw_md);
    md_req.attachments.insert(
        att_id.to_owned(),
        ResolvedImage {
            mime_type: "image/png".to_owned(),
            bytes: tiny,
        },
    );
    let err = renderer.render_markdown_to_pdf(&md_req).unwrap_err();
    assert!(
        matches!(err, RenderError::ForbiddenContent { .. }),
        "{err:?}"
    );
}

#[test]
fn deeply_nested_html_and_markdown_fail_closed_without_stack_overflow() {
    let renderer = BoundedDocumentRenderer::production();

    let deep_divs = format!("{}{}", "<div>".repeat(5_000), "</div>".repeat(5_000));
    let err = renderer
        .render_html_to_pdf(&RenderRequest::html(deep_divs))
        .unwrap_err();
    assert!(
        matches!(err, RenderError::ForbiddenContent { .. }),
        "{err:?}"
    );

    let deep_spans = format!("{}x{}", "<span>".repeat(5_000), "</span>".repeat(5_000));
    let err = renderer
        .render_html_to_pdf(&RenderRequest::html(deep_spans))
        .unwrap_err();
    assert!(
        matches!(err, RenderError::ForbiddenContent { .. }),
        "{err:?}"
    );

    let deep_quotes = format!("{}deep", "> ".repeat(200));
    let err = renderer
        .render_markdown_to_pdf(&RenderRequest::markdown(deep_quotes))
        .unwrap_err();
    assert!(
        matches!(err, RenderError::ForbiddenContent { .. }),
        "{err:?}"
    );
}

#[test]
fn multiline_markdown_html_blocks_template_tag_and_css_blocklist_fail_closed() {
    let renderer = BoundedDocumentRenderer::production();

    for bad_md in [
        "<style>\n@import \"http://evil.example/a.css\";\n</style>\n\n# Hi",
        "<script\nsrc=\"http://evil.example/x.js\"></script>\n\n# Hi",
        "<img\nsrc=\"file:///etc/passwd\">\n\n# Hi",
        "text <img\nsrc=\"file:///etc/passwd\">",
        "<template><img src=\"http://evil.example/x.png\"></template>\n",
        "hello <style>@import \"http://evil.example/a.css\";</style>",
        "<script src=\"http://evil.example/x.js\"",
        "<div data-unclosed=\"1\"",
        "hello <script>alert(1)",
        "hello <iframe src=\"https://evil.example\">",
    ] {
        let err = renderer
            .render_markdown_to_pdf(&RenderRequest::markdown(bad_md))
            .expect_err(&format!(
                "expected Markdown vector to fail closed: {bad_md}"
            ));
        assert!(
            matches!(
                err,
                RenderError::ForbiddenContent { .. } | RenderError::ForbiddenResourceUrl { .. }
            ),
            "unexpected error for `{bad_md}`: {err:?}"
        );
    }

    let tmpl_err = renderer
        .render_html_to_pdf(&RenderRequest::html(
            r#"<template><img src="http://evil.example/x.png"></template><p>ok</p>"#,
        ))
        .unwrap_err();
    assert!(
        matches!(tmpl_err, RenderError::ForbiddenContent { .. }),
        "{tmpl_err:?}"
    );

    for bad_css in [
        r#"body { background-image: image-set(url("https://evil.example/a.png") 1x); }"#,
        r#"body { background-image: -webkit-image-set("https://evil.example/a.png" 1x); }"#,
        r#"@font-face { font-family: x; src: url("https://evil.example/a.woff2"); }"#,
        r#"@namespace url("http://www.w3.org/1999/xhtml");"#,
        "body { width: expression \t\n (alert(1)); }",
    ] {
        let err = renderer
            .render_html_to_pdf(&RenderRequest::html("<p>safe</p>").with_css(bad_css))
            .expect_err(&format!("expected CSS vector to fail closed: {bad_css}"));
        assert!(
            matches!(
                err,
                RenderError::ForbiddenContent { .. } | RenderError::ForbiddenResourceUrl { .. }
            ),
            "unexpected CSS error for `{bad_css}`: {err:?}"
        );
    }
}

#[test]
fn cooperative_deadline_aborts_timeout_and_releases_semaphore_permit_promptly() {
    use crate::RenderDeadline;

    struct CooperativeSlowRenderer {
        real: PdfWriterRenderer,
    }

    impl DocumentRenderer for CooperativeSlowRenderer {
        fn render_markdown_to_pdf(&self, req: &RenderRequest) -> Result<RenderedPdf, RenderError> {
            self.real.render_markdown_to_pdf(req)
        }
        fn render_html_to_pdf(&self, req: &RenderRequest) -> Result<RenderedPdf, RenderError> {
            self.real.render_html_to_pdf(req)
        }
        fn render_with_deadline(
            &self,
            req: &RenderRequest,
            deadline: Option<&RenderDeadline>,
        ) -> Result<RenderedPdf, RenderError> {
            if req.source.contains("SPIN") {
                for _ in 0..500 {
                    if let Some(d) = deadline {
                        d.check()?;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
            self.real.render_with_deadline(req, deadline)
        }
    }

    let bounded = BoundedDocumentRenderer::new(
        CooperativeSlowRenderer {
            real: PdfWriterRenderer::default(),
        },
        RenderLimits {
            max_concurrency: 1,
            timeout: Duration::from_millis(35),
            ..RenderLimits::default()
        },
    );

    let err = bounded
        .render_markdown_to_pdf(&RenderRequest::markdown("SPIN forever"))
        .unwrap_err();
    assert!(matches!(err, RenderError::Timeout { .. }), "{err:?}");

    // Cooperative cancel aborts the worker promptly so the sole permit is immediately reusable!
    let recovered = bounded
        .render_markdown_to_pdf(&RenderRequest::markdown("# Fast Recovery"))
        .expect("single semaphore permit must be released promptly after timeout");
    assert_valid_pdf(&recovered, 1);

    // Real PdfWriterRenderer also honors tiny deadline fail-closed.
    let real_tiny = BoundedDocumentRenderer::new(
        PdfWriterRenderer::default(),
        RenderLimits {
            max_concurrency: 1,
            timeout: Duration::from_nanos(1),
            ..RenderLimits::default()
        },
    );
    let mut big_rows = String::from("<table>");
    for i in 0..400 {
        big_rows.push_str(&format!("<tr><td>R{i}</td><td>Row {i}</td></tr>"));
    }
    big_rows.push_str("</table>");
    let err = real_tiny
        .render_html_to_pdf(&RenderRequest::html(big_rows))
        .unwrap_err();
    assert!(matches!(err, RenderError::Timeout { .. }), "{err:?}");
}

#[test]
fn count_pdf_pages_uses_max_of_pages_count_and_page_leaves() {
    let forged = b"%PDF-1.7\n\
1 0 obj<< /Type /Catalog /Pages 2 0 R >>endobj\n\
2 0 obj<< /Type /Pages /Count 1 /Kids [3 0 R 4 0 R 5 0 R] >>endobj\n\
3 0 obj<< /Type /Page /Parent 2 0 R >>endobj\n\
4 0 obj<< /Type /Page /Parent 2 0 R >>endobj\n\
5 0 obj<< /Type /Page /Parent 2 0 R >>endobj\n\
xref\n0 1\n0000000000 65535 f \ntrailer<<>>\nstartxref\n10\n%%EOF\n";
    assert_eq!(count_pdf_pages(forged).unwrap(), 3);
}
