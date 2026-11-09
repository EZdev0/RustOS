//! `rustos-web` — the browser engine core of RustOS.
//!
//! A dependency-free (`core` + `alloc`) HTML/CSS/HTTP pipeline that runs both
//! inside the bare-metal kernel and in host unit tests:
//!
//! ```text
//!    Url::parse ─► http::request_bytes ─► [ kernel TCP socket ]
//!                                              │
//!        http::ResponseParser ◄────────────────┘   (incremental, chunk-safe)
//!                    │
//!                    ▼
//!              html::Document          (forgiving tokenizer, bounded depth)
//!                    │
//!        style::StyleSheet (CSS subset)
//!                    │
//!                    ▼
//!        layout::layout_document  ─►  Layout { Vec<DrawCmd> }
//!                    │
//!                    ▼
//!        compositor::blit commands into the framebuffer
//! ```
//!
//! The kernel never parses markup, never walks a DOM during a frame and never
//! allocates while painting: it replays a flat display list that was built once
//! per navigation or resize. That is what keeps the UI responsive on a software
//! framebuffer.
//!
//! # Example
//!
//! ```ignore
//! let mut engine = PageEngine::start(Url::parse("http://example.com/")?);
//! if let Some(request) = engine.take_request() {
//!     network::send(request.as_bytes());
//! }
//! while let Some(chunk) = network::poll_chunk() {
//!     engine.feed(&chunk);
//! }
//! let layout = engine.layout(LayoutOptions { width: 640, dark_mode: false });
//! ```
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![cfg_attr(not(test), deny(clippy::panic, clippy::unwrap_used))]
// Ring 0 safety rules for this crate:
// * `unsafe` is forbidden outright,
// * `panic!`/`unwrap()` are denied outside tests (a panic is a kernel panic),
// * every slice/string index is guarded by an explicit bounds check on the
//   previous line, so `clippy::indexing_slicing` is allowed (the lint cannot
//   see the guard); the hostile-input tests are the referee.

extern crate alloc;

pub mod color;
pub mod engine;
pub mod html;
pub mod http;
pub mod layout;
pub mod style;
pub mod stylesheet;
pub mod url;

pub use color::Color;
pub use engine::{error_page, FetchState, PageEngine, MAX_PAGE_BYTES, MAX_REDIRECTS};
pub use html::{Document, Node, NodeKind};
pub use http::{HttpError, Response, ResponseParser};
pub use layout::{layout_document, DrawCmd, Layout, LayoutOptions};
pub use url::{Url, UrlError};
