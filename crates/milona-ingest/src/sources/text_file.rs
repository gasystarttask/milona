//! Local plain-text file `DocumentSource`.

use std::collections::HashMap;

use async_trait::async_trait;
use milona_core::document::{RawDocument, SourceKind};
use milona_core::error::CoreError;
use milona_core::tenant::TenantContext;
use milona_core::traits::DocumentSource;

use crate::error::IngestError;

/// Reads a UTF-8 text file from the local filesystem and wraps it as a
/// [`RawDocument`] with [`SourceKind::Text`].
///
/// `location` is interpreted as a filesystem path. The `tenant_id` is not
/// used to scope the filesystem read itself (there is no tenant-partitioned
/// filesystem here) but is still required by the `DocumentSource` trait
/// signature per ROADMAP.md Phase 0.5, and is recorded in the resulting
/// document metadata for downstream audit/attribution.
#[derive(Debug, Default, Clone, Copy)]
pub struct TextFileSource;

impl TextFileSource {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DocumentSource for TextFileSource {
    async fn fetch(&self, ctx: &TenantContext, location: &str) -> Result<RawDocument, CoreError> {
        let path = location.to_string();
        let text = tokio::fs::read_to_string(&path)
            .await
            .map_err(|source| IngestError::Io {
                path: path.clone(),
                source,
            })?;

        let mut metadata = HashMap::new();
        metadata.insert("tenant_id".to_string(), ctx.tenant_id.to_string());
        metadata.insert("path".to_string(), path.clone());

        Ok(RawDocument {
            id: Default::default(),
            text,
            source_kind: SourceKind::Text,
            origin: path,
            metadata,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use milona_core::tenant::{Role, TenantContext, TenantId};
    use uuid::Uuid;

    fn ctx() -> TenantContext {
        TenantContext::new(TenantId::new(Uuid::new_v4()), Role::Member, "tester")
    }

    #[tokio::test]
    async fn fetches_text_file_with_correct_source_kind() {
        let dir = tempfile_dir();
        let file_path = dir.join("sample.txt");
        tokio::fs::write(&file_path, "hello milona\nsecond line")
            .await
            .unwrap();

        let source = TextFileSource::new();
        let ctx = ctx();
        let doc = source
            .fetch(&ctx, file_path.to_str().unwrap())
            .await
            .unwrap();

        assert_eq!(doc.source_kind, SourceKind::Text);
        assert_eq!(doc.text, "hello milona\nsecond line");
        assert_eq!(doc.origin, file_path.to_str().unwrap());
        assert_eq!(
            doc.metadata.get("tenant_id").unwrap(),
            &ctx.tenant_id.to_string()
        );

        tokio::fs::remove_file(&file_path).await.ok();
    }

    #[tokio::test]
    async fn missing_file_returns_error() {
        let source = TextFileSource::new();
        let ctx = ctx();
        let result = source
            .fetch(&ctx, "/nonexistent/path/does-not-exist.txt")
            .await;
        assert!(result.is_err());
    }

    fn tempfile_dir() -> std::path::PathBuf {
        std::env::temp_dir()
    }
}
