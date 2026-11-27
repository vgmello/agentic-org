//! The shared attachments family on the memory engine (`MemoryTx` / `MemorySnapshot`).

use std::{any::Any, collections::BTreeMap};

use org_proto::attachments::AttachmentTargetKind;
use org_store::{
    testkit::{FamilyTable, MemorySnapshot, MemoryTx},
    EngineError, Id,
};

use super::{
    ports::{AttachmentsRead, AttachmentsWrite},
    records::AttachmentRecord,
};

#[derive(Clone, Default, Debug)]
pub struct AttachmentsTables {
    attachments: BTreeMap<Id, AttachmentRecord>,
}

impl FamilyTable for AttachmentsTables {
    fn name(&self) -> &'static str {
        "attachments"
    }

    fn clone_box(&self) -> Box<dyn FamilyTable> {
        Box::new(self.clone())
    }

    fn dump(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .attachments
            .values()
            .enumerate()
            .map(|(i, row)| format!("attachments {i:06} {row:?}"))
            .collect();
        lines.sort();
        lines
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn read<R: Default>(
    tables: Option<&AttachmentsTables>,
    query: impl FnOnce(&AttachmentsTables) -> R,
) -> Result<R, EngineError> {
    Ok(tables.map(query).unwrap_or_default())
}

macro_rules! memory_read {
    ($($view:ty),+) => {$(
        impl AttachmentsRead for $view {
            fn attachment(&self, id: &Id) -> Result<Option<AttachmentRecord>, EngineError> {
                read(self.table(), |t| t.attachments.get(id).cloned())
            }

            fn attachments_for_target(
                &self,
                kind: AttachmentTargetKind,
                target_id: Option<&Id>,
                after: Option<&Id>,
                limit: usize,
            ) -> Result<Vec<AttachmentRecord>, EngineError> {
                read(self.table(), |t| {
                    t.attachments
                        .values()
                        .filter(|r| r.target_kind == kind && r.target_id.as_ref() == target_id)
                        .filter(|r| after.is_none_or(|after| r.id > *after))
                        .take(limit)
                        .cloned()
                        .collect()
                })
            }

            fn attachments(&self) -> Result<Vec<AttachmentRecord>, EngineError> {
                read(self.table(), |t| t.attachments.values().cloned().collect())
            }
        }
    )+};
}

memory_read!(MemoryTx, MemorySnapshot);

impl AttachmentsWrite for MemoryTx {
    fn put_attachment(&mut self, record: &AttachmentRecord) -> Result<(), EngineError> {
        let tables: &mut AttachmentsTables = self.table_mut()?;
        tables.attachments.insert(record.id.clone(), record.clone());
        Ok(())
    }

    fn remove_attachment(&mut self, id: &Id) -> Result<(), EngineError> {
        let tables: &mut AttachmentsTables = self.table_mut()?;
        tables.attachments.remove(id);
        Ok(())
    }
}

/// A real `width` x `height` PNG (left half red, right half blue) that the image sanitizer
/// accepts: for tests of the attachment flows in other crates.
pub fn sample_png(width: u32, height: u32) -> Vec<u8> {
    use image::codecs::png::PngEncoder;
    use image::{DynamicImage, Rgb, RgbImage};
    let pixels = RgbImage::from_fn(width, height, |x, _| {
        if x < width / 2 {
            Rgb([220, 20, 20])
        } else {
            Rgb([20, 20, 220])
        }
    });
    let mut out = Vec::new();
    DynamicImage::ImageRgb8(pixels)
        .write_with_encoder(PngEncoder::new(&mut out))
        .expect("a PNG encodes");
    out
}

/// A valid PDF of `pages` pages padded with a comment of about `padding` bytes, so a test can
/// make one as large as a transfer needs (the validator reads the page tree, not the size).
pub fn sample_pdf(pages: usize, padding: usize) -> Vec<u8> {
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", i + 3)).collect();
    let mut body = String::from("%PDF-1.7\n%\u{e2}\u{e3}\u{cf}\u{d3}\n");
    for line in 0..padding / 64 {
        body.push_str(&format!("%{line:063}\n"));
    }
    body.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    body.push_str(&format!(
        "2 0 obj\n<< /Type /Pages /Kids [{}] /Count {pages} >>\nendobj\n",
        kids.join(" ")
    ));
    for i in 0..pages {
        body.push_str(&format!(
            "{} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n",
            i + 3
        ));
    }
    let xref = body.len();
    body.push_str(&format!(
        "xref\n0 {}\ntrailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        pages + 3,
        pages + 3
    ));
    body.into_bytes()
}
