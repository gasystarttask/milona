//! `DocumentSource` implementations for Phase 1: local text files, PDFs, and
//! basic web fetch.

mod pdf_file;
mod text_file;
mod web;

pub use pdf_file::PdfFileSource;
pub use text_file::TextFileSource;
pub use web::WebSource;
