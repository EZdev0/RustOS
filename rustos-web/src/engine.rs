//! Glue layer that turns a URL plus a stream of TCP segments into a display
//! list. This is the only type the kernel's browser app has to talk to, which
//! keeps the `unsafe`/framebuffer side of the code tiny.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::html::Document;
use crate::http::{decode_text, request_bytes, HttpError, Response, ResponseParser};
use crate::layout::{layout_document, Layout, LayoutOptions};
use crate::url::Url;

/// How many redirects a single navigation may follow.
pub const MAX_REDIRECTS: u8 = 4;
/// Bodies larger than this are clipped (the kernel heap is 16 MiB).
pub const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;

/// State of a page load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchState {
    /// Nothing requested yet.
    Idle,
    /// The request bytes have to be handed to the network stack.
    Requesting,
    /// Bytes are arriving.
    Receiving,
    /// The document is parsed and ready to be laid out.
    Complete,
    /// The transfer failed; see [`PageEngine::error_message`].
    Failed,
}

/// A single in-flight (or finished) page.
#[derive(Debug)]
pub struct PageEngine {
    url: Url,
    parser: ResponseParser,
    state: FetchState,
    request_sent: bool,
    document: Option<Document>,
    layout: Option<Layout>,
    layout_width: u32,
    dark_mode: bool,
    redirects: u8,
    pending_redirect: Option<Url>,
    error: Option<HttpError>,
    received_bytes: usize,
}

impl PageEngine {
    /// Begin loading `url` (no bytes are sent until [`PageEngine::take_request`]).
    pub fn start(url: Url) -> Self {
        Self::with_redirects(url, 0)
    }

    fn with_redirects(url: Url, redirects: u8) -> Self {
        Self {
            url,
            parser: ResponseParser::new(MAX_PAGE_BYTES),
            state: FetchState::Requesting,
            request_sent: false,
            document: None,
            layout: None,
            layout_width: 0,
            dark_mode: false,
            redirects,
            pending_redirect: None,
            error: None,
            received_bytes: 0,
        }
    }

    /// Build an engine that already holds a locally generated document.
    ///
    /// Used for `about:` pages and for rendering error pages without a network
    /// round trip.
    pub fn from_html(url: Url, html: &str) -> Self {
        let mut engine = Self::with_redirects(url, 0);
        engine.state = FetchState::Complete;
        engine.request_sent = true;
        engine.document = Some(Document::parse(html));
        engine
    }

    /// The URL this engine is loading.
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Current load state.
    pub fn state(&self) -> FetchState {
        self.state
    }

    /// Number of redirects followed so far.
    pub fn redirects(&self) -> u8 {
        self.redirects
    }

    /// Raw bytes received so far (for the status bar).
    pub fn received_bytes(&self) -> usize {
        self.received_bytes
    }

    /// The parsed document, once available.
    pub fn document(&self) -> Option<&Document> {
        self.document.as_ref()
    }

    /// The decoded response, once the head has arrived.
    pub fn response(&self) -> Option<&Response> {
        let response = self.parser.response();
        if response.status == 0 {
            None
        } else {
            Some(response)
        }
    }

    /// Transfer error, if any.
    pub fn error_message(&self) -> Option<&'static str> {
        self.error.map(HttpError::message)
    }

    /// Request bytes to write to the TCP socket, exactly once.
    pub fn take_request(&mut self) -> Option<String> {
        if self.state != FetchState::Requesting || self.request_sent {
            return None;
        }
        self.request_sent = true;
        Some(request_bytes(&self.url))
    }

    /// Feed a TCP payload slice into the response decoder.
    pub fn feed(&mut self, chunk: &[u8]) {
        if matches!(self.state, FetchState::Complete | FetchState::Failed) {
            return;
        }
        if self.state == FetchState::Requesting {
            self.state = FetchState::Receiving;
        }
        self.received_bytes = self.received_bytes.saturating_add(chunk.len());
        match self.parser.feed(chunk) {
            Ok(()) => {
                if self.parser.is_complete() {
                    self.finish_document();
                }
            }
            Err(error) => {
                self.error = Some(error);
                self.state = FetchState::Failed;
            }
        }
    }

    /// Signal that the transport closed.
    pub fn finish_stream(&mut self) {
        if matches!(self.state, FetchState::Complete | FetchState::Failed) {
            return;
        }
        if self.state == FetchState::Requesting {
            // The socket closed before a single byte arrived.
            self.error = Some(HttpError::IncompleteHead);
            self.state = FetchState::Failed;
            return;
        }
        match self.parser.finish() {
            Ok(()) => self.finish_document(),
            Err(error) => {
                self.error = Some(error);
                self.state = FetchState::Failed;
            }
        }
    }

    /// If this response was a redirect, the URL to follow next.
    pub fn take_redirect(&mut self) -> Option<Url> {
        self.pending_redirect.take()
    }

    fn finish_document(&mut self) {
        let response = self.parser.response();

        if response.is_redirect() && self.redirects < MAX_REDIRECTS {
            if let Some(location) = response.location() {
                if let Ok(next) = self.url.resolve(location) {
                    let mut engine = PageEngine::with_redirects(next, self.redirects + 1);
                    engine.dark_mode = self.dark_mode;
                    *self = engine;
                    return;
                }
            }
        }

        if let Some(encoding) = response.unsupported_encoding() {
            let html = error_page(
                "Unsupported encoding",
                &alloc::format!(
                    "The server sent Content-Encoding: {encoding}, which this engine cannot decode.\n\
                     Try a plain-HTML mirror of this page."
                ),
            );
            self.document = Some(Document::parse(&html));
            self.state = FetchState::Complete;
            return;
        }

        let mime = response.mime();
        let html = match mime.as_str() {
            "text/plain" => {
                let text = decode_text(&response.body, &response.charset());
                alloc::format!("<pre>{}</pre>", escape_html(&text))
            }
            other if other.contains("html") || other.contains("xml") => {
                decode_text(&response.body, &response.charset())
            }
            other => error_page(
                "Unsupported content type",
                &alloc::format!("This engine renders HTML and plain text, not `{other}`."),
            ),
        };

        let document = Document::parse(&html);
        self.document = Some(document);
        self.layout = None;
        self.state = FetchState::Complete;
    }

    /// Lay out the document for `options`, reusing the previous result when the
    /// viewport width and theme did not change.
    pub fn layout(&mut self, options: LayoutOptions) -> Option<&Layout> {
        let width = options.width.max(64);
        let needs_layout = match &self.layout {
            None => true,
            Some(layout) => layout.width != width || self.dark_mode != options.dark_mode,
        };
        if needs_layout {
            let document = self.document.as_ref()?;
            self.layout = Some(layout_document(document, options));
            self.layout_width = width;
            self.dark_mode = options.dark_mode;
        }
        self.layout.as_ref()
    }

    /// Cached layout without recomputing (used by the paint path).
    pub fn cached_layout(&self) -> Option<&Layout> {
        self.layout.as_ref()
    }

    /// Width the cached layout was computed for.
    pub fn layout_width(&self) -> u32 {
        self.layout_width
    }

    /// A short status string for the browser chrome.
    pub fn status_text(&self) -> String {
        match self.state {
            FetchState::Idle => String::from("Idle"),
            FetchState::Requesting => String::from("Connecting..."),
            FetchState::Receiving => {
                let response = self.parser.response();
                if response.status == 0 {
                    alloc::format!("Receiving {} bytes", self.received_bytes)
                } else {
                    alloc::format!("{} - {} bytes", response.status_line(), self.received_bytes)
                }
            }
            FetchState::Complete => {
                let status = self.parser.response().status_line();
                if let Some(document) = &self.document {
                    let title = document
                        .title
                        .clone()
                        .filter(|title| !title.is_empty())
                        .unwrap_or_else(|| self.url.host().to_string());
                    return alloc::format!("{status} - {title}");
                }
                status
            }
            FetchState::Failed => alloc::format!(
                "Error: {}",
                self.error
                    .map(HttpError::message)
                    .unwrap_or("unknown error")
            ),
        }
    }

    /// Title of the document, falling back to the host name.
    pub fn title(&self) -> String {
        self.document
            .as_ref()
            .and_then(|document| document.title.clone())
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| self.url.host().to_string())
    }
}

/// Render a locally generated error page (safe subset of HTML).
pub fn error_page(title: &str, message: &str) -> String {
    alloc::format!(
        "<h2>{}</h2><p style=\"color:#a02222\">{}</p><hr><p style=\"color:#777777\">RustOS browser engine (rustos-web)</p>",
        escape_html(title),
        escape_html(message).replace('\n', "<br>"),
    )
}

/// Escape the characters that would otherwise be interpreted as markup.
pub fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 8);
    for character in input.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

/// Split a stream of received chunks into a `Vec` (helper for tests and the
/// shell's `fetch` command).
pub fn collect_chunks(chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in chunks {
        out.extend_from_slice(chunk);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn url(input: &str) -> Url {
        Url::parse(input).expect("url")
    }

    const SAMPLE_BODY: &[u8] = b"<h1>Hello</h1><p>World of RustOS</p>";

    /// The same page framed with a correct `Content-Length`.
    fn sample() -> Vec<u8> {
        let mut raw = alloc::format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n",
            SAMPLE_BODY.len()
        )
        .into_bytes();
        raw.extend_from_slice(SAMPLE_BODY);
        raw
    }

    #[test]
    fn requests_exactly_once() {
        let mut engine = PageEngine::start(url("http://example.com/"));
        assert_eq!(engine.state(), FetchState::Requesting);
        let request = engine.take_request().expect("request");
        assert!(request.starts_with("GET / HTTP/1.1\r\n"));
        assert!(engine.take_request().is_none());
    }

    #[test]
    fn streams_response_into_layout() {
        let mut engine = PageEngine::start(url("http://example.com/"));
        let _ = engine.take_request();
        // Split the response at an awkward boundary.
        let raw = sample();
        let (head, tail) = raw.split_at(20);
        engine.feed(head);
        assert_eq!(engine.state(), FetchState::Receiving);
        engine.feed(tail);
        assert_eq!(engine.state(), FetchState::Complete);
        let layout = engine
            .layout(LayoutOptions {
                width: 400,
                dark_mode: false,
            })
            .expect("layout");
        assert!(layout.height > 0);
        assert!(engine.status_text().contains("200"));
        assert_eq!(engine.title(), "example.com");
    }

    #[test]
    fn layout_is_cached_per_width() {
        let mut engine = PageEngine::start(url("http://example.com/"));
        let _ = engine.take_request();
        engine.feed(&sample());
        let first = engine
            .layout(LayoutOptions {
                width: 400,
                dark_mode: false,
            })
            .map(|layout| layout.cmds.len())
            .unwrap_or(0);
        let second = engine
            .layout(LayoutOptions {
                width: 400,
                dark_mode: false,
            })
            .map(|layout| layout.cmds.len())
            .unwrap_or(0);
        assert_eq!(first, second);
        assert_eq!(engine.layout_width(), 400);
        assert!(engine.cached_layout().is_some());
    }

    #[test]
    fn follows_redirects_once() {
        let mut engine = PageEngine::start(url("http://example.com/old"));
        let _ = engine.take_request();
        engine.feed(b"HTTP/1.1 302 Found\r\nLocation: /new\r\nContent-Length: 0\r\n\r\n");
        assert_eq!(engine.state(), FetchState::Requesting);
        assert_eq!(engine.redirects(), 1);
        assert_eq!(engine.url().path(), "/new");
        let request = engine.take_request().expect("request");
        assert!(request.starts_with("GET /new HTTP/1.1"));
    }

    #[test]
    fn redirect_loop_terminates() {
        let mut engine = PageEngine::start(url("http://example.com/0"));
        let mut hops = 0;
        loop {
            let _ = engine.take_request();
            engine.feed(b"HTTP/1.1 302 Found\r\nLocation: /same\r\nContent-Length: 0\r\n\r\n");
            hops += 1;
            if engine.state() != FetchState::Requesting {
                break;
            }
            assert!(hops < 10, "redirect loop not bounded");
        }
        assert_eq!(engine.state(), FetchState::Complete);
    }

    #[test]
    fn plain_text_becomes_preformatted() {
        let mut engine = PageEngine::start(url("http://example.com/a.txt"));
        let _ = engine.take_request();
        engine.feed(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nhello <b>x");
        engine.finish_stream();
        assert_eq!(engine.state(), FetchState::Complete);
        let layout = engine
            .layout(LayoutOptions {
                width: 300,
                dark_mode: false,
            })
            .expect("layout");
        let rendered: String = layout
            .cmds
            .iter()
            .filter_map(|cmd| match cmd {
                crate::layout::DrawCmd::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            rendered.contains("<b>x"),
            "markup must be escaped, got {rendered}"
        );
    }

    #[test]
    fn unsupported_mime_and_encoding_produce_error_pages() {
        let mut engine = PageEngine::start(url("http://example.com/a.png"));
        let _ = engine.take_request();
        engine.feed(b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 3\r\n\r\nabc");
        assert_eq!(engine.state(), FetchState::Complete);
        assert!(engine.status_text().contains("200"));

        let mut gzip = PageEngine::start(url("http://example.com/a"));
        let _ = gzip.take_request();
        gzip.feed(b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 1\r\n\r\nx");
        assert_eq!(gzip.state(), FetchState::Complete);
        let document = gzip.document().expect("document");
        assert!(document
            .root
            .text_content()
            .contains("Unsupported encoding"));
    }

    #[test]
    fn truncated_stream_marks_failure_and_truncation() {
        let mut engine = PageEngine::start(url("http://example.com/"));
        let _ = engine.take_request();
        engine.feed(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nshort");
        engine.finish_stream();
        assert_eq!(engine.state(), FetchState::Complete);
        let response = engine.response().expect("response");
        assert!(response.truncated);

        let mut dead = PageEngine::start(url("http://example.com/"));
        dead.finish_stream();
        assert_eq!(dead.state(), FetchState::Failed);
        assert!(dead.error_message().is_some());
    }

    #[test]
    fn malformed_response_fails_cleanly() {
        let mut engine = PageEngine::start(url("http://example.com/"));
        let _ = engine.take_request();
        engine.feed(b"garbage\r\n\r\n");
        assert_eq!(engine.state(), FetchState::Failed);
        assert!(engine.status_text().starts_with("Error:"));
    }

    #[test]
    fn from_html_marks_complete() {
        let engine = PageEngine::from_html(url("http://localhost/"), "<h1>Local</h1>");
        assert_eq!(engine.state(), FetchState::Complete);
        assert!(engine.document().is_some());
    }

    #[test]
    fn escapes_markup_in_error_pages() {
        let page = error_page("<script>", "a & b");
        assert!(!page.contains("<script>"));
        assert!(page.contains("&lt;script&gt;"));
        assert!(page.contains("a &amp; b"));
        assert_eq!(escape_html("\"x\""), "&quot;x&quot;");
    }

    #[test]
    fn collects_chunks() {
        let chunks = vec![Vec::from(&b"a"[..]), Vec::from(&b"bc"[..])];
        assert_eq!(collect_chunks(&chunks), b"abc");
    }
}
