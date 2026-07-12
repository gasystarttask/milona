//! Local PDF file `DocumentSource`, using the pure-Rust `pdf-extract` crate
//! (no system PDFium dependency, unlike `pdfium-render` recommended in
//! ROADMAP.md — substituted here to avoid a native-library dependency in
//! this sandbox).

use std::collections::HashMap;

use async_trait::async_trait;
use milona_core::document::{RawDocument, SourceKind};
use milona_core::error::CoreError;
use milona_core::tenant::TenantContext;
use milona_core::traits::DocumentSource;

use crate::error::IngestError;

/// Extracts text from a local PDF file at `location` (a filesystem path).
#[derive(Debug, Default, Clone, Copy)]
pub struct PdfFileSource;

impl PdfFileSource {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DocumentSource for PdfFileSource {
    async fn fetch(&self, ctx: &TenantContext, location: &str) -> Result<RawDocument, CoreError> {
        let path = location.to_string();
        let extract_path = path.clone();

        // pdf-extract is synchronous/CPU-bound; run it off the async
        // executor thread so a large PDF doesn't stall the Tokio runtime.
        let text = tokio::task::spawn_blocking(move || pdf_extract::extract_text(&extract_path))
            .await
            .map_err(|join_err| IngestError::Pdf {
                origin: path.clone(),
                message: format!("blocking task panicked: {join_err}"),
            })?
            .map_err(|source| IngestError::Pdf {
                origin: path.clone(),
                message: source.to_string(),
            })?;

        let mut metadata = HashMap::new();
        metadata.insert("tenant_id".to_string(), ctx.tenant_id.to_string());
        metadata.insert("path".to_string(), path.clone());

        Ok(RawDocument {
            id: Default::default(),
            text,
            source_kind: SourceKind::Pdf,
            origin: path,
            metadata,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use milona_core::tenant::{Role, TenantContext, TenantId};
    use std::io::Write;
    use uuid::Uuid;

    fn ctx() -> TenantContext {
        TenantContext::new(TenantId::new(Uuid::new_v4()), Role::Member, "tester")
    }

    /// Hand-rolled minimal single-page PDF containing the literal text
    /// "Hello Milona PDF" via a single `Tj` show-text operator, so the test
    /// has no dependency on a PDF-writing crate.
    fn minimal_pdf_bytes() -> Vec<u8> {
        let content_stream = b"BT /F1 24 Tf 72 712 Td (Hello Milona PDF) Tj ET";
        let mut pdf = Vec::new();
        pdf.extend_from_slice(b"%PDF-1.4\n");

        let mut offsets = Vec::new();

        offsets.push(pdf.len());
        pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

        offsets.push(pdf.len());
        pdf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");

        offsets.push(pdf.len());
        pdf.extend_from_slice(
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 4 0 R >> >> /MediaBox [0 0 612 792] /Contents 5 0 R >>\nendobj\n",
        );

        offsets.push(pdf.len());
        pdf.extend_from_slice(
            b"4 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
        );

        offsets.push(pdf.len());
        let stream_header = format!("5 0 obj\n<< /Length {} >>\nstream\n", content_stream.len());
        pdf.extend_from_slice(stream_header.as_bytes());
        pdf.extend_from_slice(content_stream);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");

        let xref_offset = pdf.len();
        let mut xref = format!("xref\n0 {}\n", offsets.len() + 1);
        xref.push_str("0000000000 65535 f \n");
        for off in &offsets {
            xref.push_str(&format!("{:010} 00000 n \n", off));
        }
        pdf.extend_from_slice(xref.as_bytes());
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF",
                offsets.len() + 1,
                xref_offset
            )
            .as_bytes(),
        );

        pdf
    }

    #[tokio::test]
    async fn fetches_pdf_with_correct_source_kind_and_text() {
        let mut path = std::env::temp_dir();
        path.push(format!("milona-test-{}.pdf", Uuid::new_v4()));
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(&minimal_pdf_bytes()).unwrap();
        }

        let source = PdfFileSource::new();
        let ctx = ctx();
        let doc = source.fetch(&ctx, path.to_str().unwrap()).await.unwrap();

        assert_eq!(doc.source_kind, SourceKind::Pdf);
        assert!(
            doc.text.contains("Hello Milona PDF"),
            "unexpected extracted text: {:?}",
            doc.text
        );

        std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn missing_pdf_returns_error() {
        let source = PdfFileSource::new();
        let ctx = ctx();
        let result = source
            .fetch(&ctx, "/nonexistent/path/does-not-exist.pdf")
            .await;
        assert!(result.is_err());
    }
}
