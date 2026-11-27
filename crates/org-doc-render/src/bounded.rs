//! Concrete pure-Rust [`PdfWriterRenderer`] and fail-closed [`BoundedDocumentRenderer`]
//! enforcing input size, air-gap sanitization, bounded concurrency (`max = 2`), wall-clock
//! timeout (`<= 10s`), output magic/trailer validation, and `<= 100` page cap.

use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::doc::{html_to_document, markdown_to_document};
use crate::layout::paginate;
use crate::model::{DocumentRenderer, RenderError, RenderLimits, RenderRequest, RenderedPdf};
use crate::pdf::{count_pdf_pages, emit_pdf};
use crate::sanitize::audit_request;

/// Concrete pure-Rust [`DocumentRenderer`] backed by Typst's `pdf-writer`, `pulldown-cmark`,
/// `scraper`, `image`, and `flate2` — zero Chrome, zero filesystem I/O, zero network fetches.
#[derive(Clone, Debug)]
pub struct PdfWriterRenderer {
    max_pages: u32,
}

impl Default for PdfWriterRenderer {
    fn default() -> Self {
        Self {
            max_pages: RenderLimits::default().max_pages,
        }
    }
}

impl PdfWriterRenderer {
    /// Creates a renderer with an explicit page cap.
    pub fn with_max_pages(max_pages: u32) -> Self {
        Self {
            max_pages: max_pages.max(1),
        }
    }

    fn render_with_deadline(
        &self,
        request: &RenderRequest,
        deadline: Option<Instant>,
    ) -> Result<RenderedPdf, RenderError> {
        let doc = match request.source_kind {
            crate::model::SourceKind::Markdown => markdown_to_document(
                &request.source,
                request.css.as_deref(),
                request.title.as_deref(),
                &request.attachments,
            )?,
            crate::model::SourceKind::Html => html_to_document(
                &request.source,
                request.css.as_deref(),
                request.title.as_deref(),
                &request.attachments,
            )?,
        };
        let plan = paginate(doc, self.max_pages, deadline)?;
        emit_pdf(&plan)
    }
}

impl DocumentRenderer for PdfWriterRenderer {
    fn render_markdown_to_pdf(&self, request: &RenderRequest) -> Result<RenderedPdf, RenderError> {
        let mut req = request.clone();
        req.source_kind = crate::model::SourceKind::Markdown;
        self.render_with_deadline(&req, None)
    }

    fn render_html_to_pdf(&self, request: &RenderRequest) -> Result<RenderedPdf, RenderError> {
        let mut req = request.clone();
        req.source_kind = crate::model::SourceKind::Html;
        self.render_with_deadline(&req, None)
    }
}

/// Counting semaphore with RAII permit release (even across panics).
struct Semaphore {
    state: Mutex<usize>,
    ready: Condvar,
}

impl Semaphore {
    fn new(permits: usize) -> Self {
        Self {
            state: Mutex::new(permits.max(1)),
            ready: Condvar::new(),
        }
    }

    fn acquire_until(self: &Arc<Self>, deadline: Instant) -> Result<SemaphorePermit, RenderError> {
        let mut avail = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while *avail == 0 {
            let now = Instant::now();
            if now >= deadline {
                return Err(RenderError::Busy);
            }
            let wait = deadline.saturating_duration_since(now);
            let (guard, timeout) = self
                .ready
                .wait_timeout(avail, wait)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            avail = guard;
            if *avail == 0 && timeout.timed_out() {
                return Err(RenderError::Busy);
            }
        }
        *avail -= 1;
        Ok(SemaphorePermit {
            sem: Arc::clone(self),
        })
    }
}

struct SemaphorePermit {
    sem: Arc<Semaphore>,
}

impl Drop for SemaphorePermit {
    fn drop(&mut self) {
        let mut avail = self
            .sem
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *avail += 1;
        self.sem.ready.notify_one();
    }
}

/// Fail-closed security, concurrency, timeout, and output-bounds wrapper around any
/// [`DocumentRenderer`].
pub struct BoundedDocumentRenderer<R: DocumentRenderer + 'static = PdfWriterRenderer> {
    inner: Arc<R>,
    limits: RenderLimits,
    slots: Arc<Semaphore>,
}

impl BoundedDocumentRenderer<PdfWriterRenderer> {
    /// Creates the default production renderer (`<= 2 MiB` input, `<= 100` pages, `max = 2`
    /// concurrent workers, `<= 10s` timeout).
    pub fn production() -> Self {
        let limits = RenderLimits::default();
        Self::new(PdfWriterRenderer::with_max_pages(limits.max_pages), limits)
    }
}

impl<R: DocumentRenderer + 'static> BoundedDocumentRenderer<R> {
    /// Wraps `inner` with strict [`RenderLimits`].
    pub fn new(inner: R, limits: RenderLimits) -> Self {
        Self {
            inner: Arc::new(inner),
            limits,
            slots: Arc::new(Semaphore::new(limits.max_concurrency)),
        }
    }

    /// Returns the configured [`RenderLimits`].
    pub fn limits(&self) -> RenderLimits {
        self.limits
    }

    fn execute(&self, request: &RenderRequest) -> Result<RenderedPdf, RenderError> {
        audit_request(request, self.limits.max_input_bytes)?;

        let deadline = Instant::now() + self.limits.timeout;
        let permit = self.slots.acquire_until(deadline)?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(timeout_err(self.limits.timeout));
        }

        let rendered = self.run_bounded_worker(request.clone(), permit, remaining)?;
        self.validate_output(rendered)
    }

    fn run_bounded_worker(
        &self,
        request: RenderRequest,
        permit: SemaphorePermit,
        remaining: Duration,
    ) -> Result<RenderedPdf, RenderError> {
        let inner = Arc::clone(&self.inner);
        let timeout_limit = self.limits.timeout;
        let (tx, rx) = mpsc::sync_channel(1);
        let spawn = std::thread::Builder::new()
            .name("org-doc-render".into())
            .spawn(move || {
                // Hold the RAII permit for the entire lifetime of the worker thread so a timed-out
                // render never allows unbounded background CPU threads to pile up.
                let _permit = permit;
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    inner.render(&request)
                }))
                .unwrap_or_else(|_| {
                    Err(RenderError::RenderFailed(
                        "document renderer panicked unexpectedly".to_owned(),
                    ))
                });
                let _ = tx.send(outcome);
            });
        if let Err(err) = spawn {
            return Err(RenderError::RenderFailed(format!(
                "could not start render worker thread: {err}"
            )));
        }

        match rx.recv_timeout(remaining) {
            Ok(Ok(pdf)) => Ok(pdf),
            Ok(Err(RenderError::Timeout { .. })) | Err(mpsc::RecvTimeoutError::Timeout) => {
                Err(timeout_err(timeout_limit))
            }
            Ok(Err(err)) => Err(err),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(RenderError::RenderFailed(
                "render worker terminated without replying".to_owned(),
            )),
        }
    }

    fn validate_output(&self, pdf: RenderedPdf) -> Result<RenderedPdf, RenderError> {
        if pdf.bytes.len() > self.limits.max_output_bytes {
            return Err(RenderError::OutputTooLarge {
                actual: pdf.bytes.len(),
                max: self.limits.max_output_bytes,
            });
        }
        let verified_pages = count_pdf_pages(&pdf.bytes)?;
        if verified_pages > self.limits.max_pages {
            return Err(RenderError::PageLimitExceeded {
                actual: verified_pages,
                max: self.limits.max_pages,
            });
        }
        Ok(RenderedPdf {
            byte_count: pdf.bytes.len() as u64,
            page_count: verified_pages,
            bytes: pdf.bytes,
        })
    }
}

fn timeout_err(d: Duration) -> RenderError {
    RenderError::Timeout {
        timeout_ms: u64::try_from(d.as_millis()).unwrap_or(u64::MAX),
    }
}

impl<R: DocumentRenderer + 'static> DocumentRenderer for BoundedDocumentRenderer<R> {
    fn render_markdown_to_pdf(&self, request: &RenderRequest) -> Result<RenderedPdf, RenderError> {
        let mut req = request.clone();
        req.source_kind = crate::model::SourceKind::Markdown;
        self.execute(&req)
    }

    fn render_html_to_pdf(&self, request: &RenderRequest) -> Result<RenderedPdf, RenderError> {
        let mut req = request.clone();
        req.source_kind = crate::model::SourceKind::Html;
        self.execute(&req)
    }
}
