//! Basic web-page `DocumentSource` using `reqwest` for fetching and
//! `scraper` for HTML text extraction.
//!
//! ROADMAP.md recommends `reqwest` + `scraper` + `dom_smoothie` (for
//! "main-content" readability-style extraction, similar to Mozilla's
//! Readability). `dom_smoothie` was impractical to wire into this sandbox in
//! the time budgeted for Phase 1, so this implementation substitutes a
//! simpler heuristic: it drops `<script>`/`<style>`/`<noscript>` elements,
//! then concatenates the text nodes of the document body (falling back to
//! the whole document if no `<body>` is present), collapsing whitespace.
//! This is strictly less sophisticated than a readability algorithm (it
//! will include nav/boilerplate text a "main content" extractor would
//! drop), but is a correct, dependency-light text extraction pass. Swapping
//! in `dom_smoothie` later only touches this module.

use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use milona_core::document::{RawDocument, SourceKind};
use milona_core::error::CoreError;
use milona_core::tenant::TenantContext;
use milona_core::traits::DocumentSource;
use scraper::{Html, Selector};

use crate::error::IngestError;

/// Fetches a web page at `location` (an absolute HTTP/HTTPS URL) and
/// extracts its visible text.
pub struct WebSource {
    client: reqwest::Client,
}

impl WebSource {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("milona-ingest/0.1")
            .build()
            .expect("reqwest client build with static config should not fail");
        Self { client }
    }

    /// Construct with a caller-supplied client, e.g. one preconfigured with
    /// a proxy or a mock base URL in tests.
    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// Extracts readable-ish text from an HTML document. Pure, synchronous,
    /// and unit-testable independent of any network call.
    pub fn extract_text(html: &str) -> String {
        let document = Html::parse_document(html);

        let noise_selector =
            Selector::parse("script, style, noscript").expect("static selector is valid");

        let body_selector = Selector::parse("body").expect("static selector is valid");

        let root = document
            .select(&body_selector)
            .next()
            .unwrap_or(document.root_element());

        let noise_nodes: std::collections::HashSet<_> = document
            .select(&noise_selector)
            .flat_map(|el| el.descendants().map(|n| n.id()).chain([el.id()]))
            .collect();

        let mut text_parts = Vec::new();
        for node in root.descendants() {
            if noise_nodes.contains(&node.id()) {
                continue;
            }
            if let Some(text) = node.value().as_text() {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    text_parts.push(trimmed.to_string());
                }
            }
        }

        text_parts.join(" ")
    }

    fn extract_title(html: &str) -> Option<String> {
        let document = Html::parse_document(html);
        let title_selector = Selector::parse("title").ok()?;
        document
            .select(&title_selector)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
    }
}

impl Default for WebSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DocumentSource for WebSource {
    async fn fetch(&self, ctx: &TenantContext, location: &str) -> Result<RawDocument, CoreError> {
        let url = reqwest::Url::parse(location)
            .map_err(|e| IngestError::InvalidUrl(format!("{location}: {e}")))?;

        let response =
            self.client
                .get(url.clone())
                .send()
                .await
                .map_err(|e| IngestError::Http {
                    url: location.to_string(),
                    message: e.to_string(),
                })?;

        let status = response.status();
        if !status.is_success() {
            return Err(IngestError::Http {
                url: location.to_string(),
                message: format!("unexpected status {status}"),
            }
            .into());
        }

        let html = response.text().await.map_err(|e| IngestError::Http {
            url: location.to_string(),
            message: e.to_string(),
        })?;

        let text = Self::extract_text(&html);
        let title = Self::extract_title(&html);

        let mut metadata = HashMap::new();
        metadata.insert("tenant_id".to_string(), ctx.tenant_id.to_string());
        metadata.insert("url".to_string(), location.to_string());
        if let Some(title) = title {
            metadata.insert("title".to_string(), title);
        }

        Ok(RawDocument {
            id: Default::default(),
            text,
            source_kind: SourceKind::Web,
            origin: location.to_string(),
            metadata,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use milona_core::tenant::{Role, TenantContext, TenantId};
    use uuid::Uuid;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ctx() -> TenantContext {
        TenantContext::new(TenantId::new(Uuid::new_v4()), Role::Member, "tester")
    }

    #[test]
    fn extract_text_strips_script_and_style() {
        let html = r#"
            <html>
                <head><title>Test Page</title><style>body { color: red; }</style></head>
                <body>
                    <script>alert('should not appear');</script>
                    <h1>Hello Milona</h1>
                    <p>This is a paragraph.</p>
                </body>
            </html>
        "#;

        let text = WebSource::extract_text(html);
        assert!(text.contains("Hello Milona"));
        assert!(text.contains("This is a paragraph."));
        assert!(!text.contains("alert"));
        assert!(!text.contains("color: red"));
    }

    #[test]
    fn extract_title_reads_title_tag() {
        let html = "<html><head><title>My Title</title></head><body></body></html>";
        assert_eq!(WebSource::extract_title(html), Some("My Title".to_string()));
    }

    #[tokio::test]
    async fn fetch_returns_raw_document_with_web_source_kind() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/page"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "<html><head><title>Milona</title></head><body><p>Ingested body text</p></body></html>",
            ))
            .mount(&mock_server)
            .await;

        let source = WebSource::new();
        let ctx = ctx();
        let url = format!("{}/page", mock_server.uri());
        let doc = source.fetch(&ctx, &url).await.unwrap();

        assert_eq!(doc.source_kind, SourceKind::Web);
        assert!(doc.text.contains("Ingested body text"));
        assert_eq!(doc.origin, url);
        assert_eq!(doc.metadata.get("title").unwrap(), "Milona");
        assert_eq!(
            doc.metadata.get("tenant_id").unwrap(),
            &ctx.tenant_id.to_string()
        );
    }

    #[tokio::test]
    async fn fetch_propagates_http_error_status() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/missing"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&mock_server)
            .await;

        let source = WebSource::new();
        let ctx = ctx();
        let url = format!("{}/missing", mock_server.uri());
        let result = source.fetch(&ctx, &url).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn fetch_rejects_invalid_url() {
        let source = WebSource::new();
        let ctx = ctx();
        let result = source.fetch(&ctx, "not-a-url").await;
        assert!(result.is_err());
    }
}
