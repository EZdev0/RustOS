//! Incremental HTTP/1.1 response decoder and request encoder.
//!
//! The network stack in the kernel hands us *raw TCP segments* in arbitrary
//! slices: a single `recv` may end in the middle of the status line, of a
//! chunk-size line, or of a header. [`ResponseParser`] therefore never assumes
//! message boundaries and never allocates more than the configured caps.
//!
//! Supported framing: `Content-Length`, `Transfer-Encoding: chunked` and
//! read-until-close. Compression is reported as unsupported (the kernel has no
//! inflate implementation) instead of producing binary garbage.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::url::Url;

/// Hard cap for the status line plus all headers.
pub const MAX_HEADER_BYTES: usize = 16 * 1024;
/// Default cap for a decoded response body (the browser can lower this).
pub const DEFAULT_MAX_BODY_BYTES: usize = 1024 * 1024;

const USER_AGENT: &str = "RustOS/0.3 (rustos-web; software framebuffer)";

/// Error conditions detected while decoding a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpError {
    /// The status line did not start with `HTTP/1.x`.
    UnsupportedVersion,
    /// The status line was not `<version> <code> [reason]`.
    MalformedStatusLine,
    /// Headers exceeded [`MAX_HEADER_BYTES`].
    HeadersTooLarge,
    /// A chunk-size line was not hexadecimal.
    BadChunkSize,
    /// The chunk framing was corrupted.
    BadChunk,
    /// The transport closed before the response head was complete.
    IncompleteHead,
}

impl HttpError {
    /// Human readable, user facing description.
    pub fn message(self) -> &'static str {
        match self {
            HttpError::UnsupportedVersion => "unsupported HTTP version",
            HttpError::MalformedStatusLine => "malformed status line",
            HttpError::HeadersTooLarge => "response headers too large",
            HttpError::BadChunkSize => "bad chunk size",
            HttpError::BadChunk => "corrupted chunked encoding",
            HttpError::IncompleteHead => "connection closed before headers",
        }
    }
}

/// A decoded HTTP response.
#[derive(Debug, Clone, Default)]
pub struct Response {
    /// Numeric status code (`200`, `404`, ...).
    pub status: u16,
    /// Reason phrase as sent by the server (may be empty).
    pub reason: String,
    /// Headers, names lower-cased, in wire order.
    pub headers: Vec<(String, String)>,
    /// Decoded body (de-chunked, no transfer encoding).
    pub body: Vec<u8>,
    /// True once the message is fully framed.
    pub complete: bool,
    /// True when the body was cut because a size cap was hit.
    pub truncated: bool,
}

impl Response {
    /// Case-insensitive header lookup returning the first match.
    pub fn header(&self, name: &str) -> Option<&str> {
        let needle = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(key, _)| *key == needle)
            .map(|(_, value)| value.as_str())
    }

    /// `Content-Type` header value, if present.
    pub fn content_type(&self) -> Option<&str> {
        self.header("content-type")
    }

    /// The MIME type with parameters stripped and lower-cased.
    pub fn mime(&self) -> String {
        let raw = self.content_type().unwrap_or("text/html");
        let mime = raw
            .split(';')
            .next()
            .unwrap_or("text/html")
            .trim()
            .to_ascii_lowercase();
        if mime.is_empty() {
            "text/html".to_string()
        } else {
            mime
        }
    }

    /// Charset parameter from `Content-Type`, lower-cased (defaults to `utf-8`).
    pub fn charset(&self) -> String {
        let raw = self.content_type().unwrap_or("");
        for parameter in raw.split(';').skip(1) {
            let parameter = parameter.trim();
            if let Some((key, value)) = parameter.split_once('=') {
                if key.trim().eq_ignore_ascii_case("charset") {
                    let value = value.trim().trim_matches('"');
                    if !value.is_empty() {
                        return value.to_ascii_lowercase();
                    }
                }
            }
        }
        "utf-8".to_string()
    }

    /// True for the redirect status codes this client follows.
    pub fn is_redirect(&self) -> bool {
        matches!(self.status, 301 | 302 | 303 | 307 | 308)
    }

    /// `Location` header for redirects.
    pub fn location(&self) -> Option<&str> {
        self.header("location")
    }

    /// True when the server used a content coding we cannot decode.
    pub fn unsupported_encoding(&self) -> Option<&str> {
        let encoding = self.header("content-encoding")?;
        let encoding = encoding.trim();
        if encoding.is_empty() || encoding.eq_ignore_ascii_case("identity") {
            None
        } else {
            Some(encoding)
        }
    }

    /// Body decoded as text using the declared charset (latin-1 or UTF-8).
    pub fn text(&self) -> String {
        decode_text(&self.body, &self.charset())
    }

    /// One-line status for the browser chrome, e.g. `HTTP 200 OK`.
    pub fn status_line(&self) -> String {
        if self.reason.is_empty() {
            alloc::format!("HTTP {}", self.status)
        } else {
            alloc::format!("HTTP {} {}", self.status, self.reason)
        }
    }
}

/// Body framing strategy inferred from the response head.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyMode {
    Length(usize),
    Chunked,
    UntilClose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChunkState {
    Size,
    Data(usize),
    /// Expect the CRLF that terminates a chunk's data.
    DataTail,
    /// Saw the `\r` of the terminating CRLF, waiting for the `\n`.
    DataTailLineFeed,
    Trailers,
    Done,
}

/// Streaming response decoder. Feed it every received slice, then call
/// [`ResponseParser::finish`] when the socket closes.
#[derive(Debug)]
pub struct ResponseParser {
    head: Vec<u8>,
    head_complete: bool,
    response: Response,
    mode: BodyMode,
    chunk: ChunkState,
    line: Vec<u8>,
    max_body: usize,
    error: Option<HttpError>,
}

impl Default for ResponseParser {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_BODY_BYTES)
    }
}

impl ResponseParser {
    /// Create a parser whose decoded body is capped at `max_body` bytes.
    pub fn new(max_body: usize) -> Self {
        Self {
            head: Vec::with_capacity(512),
            head_complete: false,
            response: Response::default(),
            mode: BodyMode::UntilClose,
            chunk: ChunkState::Size,
            line: Vec::new(),
            max_body: max_body.max(1024),
            error: None,
        }
    }

    /// The response decoded so far. Valid even before the message completes.
    pub fn response(&self) -> &Response {
        &self.response
    }

    /// The last error, if the stream turned out to be malformed.
    pub fn error(&self) -> Option<HttpError> {
        self.error
    }

    /// True once the message was fully framed (or truncated by a cap).
    pub fn is_complete(&self) -> bool {
        self.response.complete
    }

    /// Feed a raw TCP payload slice.
    pub fn feed(&mut self, data: &[u8]) -> Result<(), HttpError> {
        if self.error.is_some() {
            return Err(self.error.unwrap_or(HttpError::BadChunk));
        }
        if self.response.complete || data.is_empty() {
            return Ok(());
        }
        let result = self.feed_inner(data);
        if let Err(error) = result {
            self.error = Some(error);
            return Err(error);
        }
        Ok(())
    }

    fn feed_inner(&mut self, data: &[u8]) -> Result<(), HttpError> {
        if !self.head_complete {
            self.head.extend_from_slice(data);
            if self.head.len() > MAX_HEADER_BYTES {
                return Err(HttpError::HeadersTooLarge);
            }
            let end = match find_header_end(&self.head) {
                Some(index) => index,
                None => return Ok(()),
            };
            let head = self.head[..end].to_vec();
            let rest = self.head[end + 4..].to_vec();
            self.head.clear();
            self.head.shrink_to_fit();
            self.parse_head(&head)?;
            self.head_complete = true;
            if rest.is_empty() {
                return Ok(());
            }
            return self.feed_body(&rest);
        }
        self.feed_body(data)
    }

    /// Called when the transport is closed; finalises the message.
    pub fn finish(&mut self) -> Result<(), HttpError> {
        if self.response.complete {
            return Ok(());
        }
        if !self.head_complete {
            self.error = Some(HttpError::IncompleteHead);
            return Err(HttpError::IncompleteHead);
        }
        // Connection closed: whatever we framed so far is the final body.
        match self.mode {
            BodyMode::Chunked if self.chunk != ChunkState::Done => {
                self.response.truncated = true;
            }
            BodyMode::Length(remaining) if remaining > 0 => {
                self.response.truncated = true;
            }
            _ => {}
        }
        self.response.complete = true;
        Ok(())
    }

    fn parse_head(&mut self, head: &[u8]) -> Result<(), HttpError> {
        let mut lines = split_lines(head).into_iter();
        let status_line = lines.next().ok_or(HttpError::MalformedStatusLine)?;
        let status_line = String::from_utf8_lossy(status_line).into_owned();
        let mut parts = status_line.splitn(3, ' ');
        let version = parts.next().ok_or(HttpError::MalformedStatusLine)?;
        if !version.starts_with("HTTP/1.") {
            return Err(HttpError::UnsupportedVersion);
        }
        let code = parts.next().ok_or(HttpError::MalformedStatusLine)?;
        if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_digit()) {
            return Err(HttpError::MalformedStatusLine);
        }
        let status: u16 = code.parse().map_err(|_| HttpError::MalformedStatusLine)?;
        self.response.status = status;
        self.response.reason = parts.next().unwrap_or("").trim().to_string();

        let mut content_length: Option<usize> = None;
        let mut chunked = false;
        for line in lines {
            if line.is_empty() {
                continue;
            }
            let line = String::from_utf8_lossy(line);
            let (name, value) = match line.split_once(':') {
                Some(parts) => parts,
                None => continue,
            };
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            match name.as_str() {
                "content-length" => {
                    // A malformed length is ignored; framing falls back to
                    // read-until-close, which cannot hang the UI.
                    if let Ok(parsed) = value.parse::<usize>() {
                        content_length = Some(parsed);
                    }
                }
                "transfer-encoding" => {
                    // Repeated headers are OR-ed, as required for `chunked`.
                    chunked |= value.to_ascii_lowercase().contains("chunked");
                }
                _ => {}
            }
            self.response.headers.push((name, value));
        }

        // Bodyless responses (204/304/1xx) are complete immediately.
        if status == 204 || status == 304 || (100..200).contains(&status) {
            self.response.complete = true;
            self.chunk = ChunkState::Done;
            self.mode = BodyMode::Length(0);
            return Ok(());
        }

        self.mode = if chunked {
            BodyMode::Chunked
        } else if let Some(length) = content_length {
            BodyMode::Length(length)
        } else {
            BodyMode::UntilClose
        };

        if let BodyMode::Length(0) = self.mode {
            self.response.complete = true;
        }
        Ok(())
    }

    fn feed_body(&mut self, data: &[u8]) -> Result<(), HttpError> {
        match self.mode {
            BodyMode::Length(remaining) => {
                let take = remaining.min(data.len());
                self.append_body(&data[..take]);
                let left = remaining - take;
                self.mode = BodyMode::Length(left);
                if left == 0 || self.response.truncated {
                    self.response.complete = true;
                }
                // Anything past `Content-Length` is protocol garbage; drop it.
                Ok(())
            }
            BodyMode::UntilClose => {
                self.append_body(data);
                Ok(())
            }
            BodyMode::Chunked => self.decode_chunked(data),
        }
    }

    fn append_body(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let used = self.response.body.len();
        let capacity = self.max_body.saturating_sub(used);
        if capacity == 0 {
            self.response.truncated = true;
            self.response.complete = true;
            return;
        }
        if data.len() > capacity {
            self.response.body.extend_from_slice(&data[..capacity]);
            self.response.truncated = true;
            self.response.complete = true;
        } else {
            self.response.body.extend_from_slice(data);
        }
    }

    fn decode_chunked(&mut self, data: &[u8]) -> Result<(), HttpError> {
        let mut index = 0;
        while index < data.len() && !self.response.complete {
            match self.chunk {
                ChunkState::Size => {
                    let slice = &data[index..];
                    match slice.iter().position(|&byte| byte == b'\n') {
                        Some(position) => {
                            self.line.extend_from_slice(&slice[..position]);
                            let size = parse_chunk_size(&self.line)?;
                            self.line.clear();
                            index += position + 1;
                            self.chunk = if size == 0 {
                                ChunkState::Trailers
                            } else {
                                ChunkState::Data(size)
                            };
                        }
                        None => {
                            self.line.extend_from_slice(slice);
                            if self.line.len() > 64 {
                                return Err(HttpError::BadChunkSize);
                            }
                            index = data.len();
                        }
                    }
                }
                ChunkState::Data(remaining) => {
                    let available = data.len() - index;
                    let take = remaining.min(available);
                    self.append_body(&data[index..index + take]);
                    index += take;
                    let left = remaining - take;
                    self.chunk = if left == 0 {
                        ChunkState::DataTail
                    } else {
                        ChunkState::Data(left)
                    };
                }
                ChunkState::DataTail => match data[index] {
                    b'\r' => {
                        index += 1;
                        self.chunk = ChunkState::DataTailLineFeed;
                    }
                    b'\n' => {
                        index += 1;
                        self.chunk = ChunkState::Size;
                    }
                    _ => return Err(HttpError::BadChunk),
                },
                ChunkState::DataTailLineFeed => {
                    if data[index] == b'\n' {
                        index += 1;
                        self.chunk = ChunkState::Size;
                    } else {
                        return Err(HttpError::BadChunk);
                    }
                }
                ChunkState::Trailers => {
                    let slice = &data[index..];
                    match slice.iter().position(|&byte| byte == b'\n') {
                        Some(position) => {
                            let mut line: &[u8] = &slice[..position];
                            if let Some(stripped) = line.strip_suffix(b"\r") {
                                line = stripped;
                            }
                            index += position + 1;
                            if line.is_empty() {
                                self.chunk = ChunkState::Done;
                                self.response.complete = true;
                            }
                        }
                        None => {
                            self.line.extend_from_slice(slice);
                            if self.line.len() > MAX_HEADER_BYTES {
                                return Err(HttpError::HeadersTooLarge);
                            }
                            index = data.len();
                        }
                    }
                }
                ChunkState::Done => {
                    self.response.complete = true;
                    return Ok(());
                }
            }
        }
        Ok(())
    }
}

fn find_header_end(haystack: &[u8]) -> Option<usize> {
    haystack.windows(4).position(|window| window == b"\r\n\r\n")
}

fn split_lines(head: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    for line in head.split(|&byte| byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        lines.push(line);
    }
    lines
}

fn parse_chunk_size(line: &[u8]) -> Result<usize, HttpError> {
    // The size line is accumulated up to and including its terminator.
    let mut line = line;
    while let Some(stripped) = line
        .strip_suffix(b"\n")
        .or_else(|| line.strip_suffix(b"\r"))
    {
        line = stripped;
    }
    let digits: &[u8] = match line.iter().position(|&byte| byte == b';') {
        Some(extension) => &line[..extension],
        None => line,
    };
    let mut value: usize = 0;
    let mut saw_digit = false;
    for &byte in digits {
        let nibble = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            b' ' | b'\t' => continue,
            _ => return Err(HttpError::BadChunkSize),
        };
        saw_digit = true;
        value = value
            .checked_mul(16)
            .and_then(|acc| acc.checked_add(nibble as usize))
            .ok_or(HttpError::BadChunkSize)?;
    }
    if saw_digit {
        Ok(value)
    } else {
        Err(HttpError::BadChunkSize)
    }
}

/// Decode a body slice to text, honouring the declared charset.
pub fn decode_text(body: &[u8], charset: &str) -> String {
    let charset = charset.trim().to_ascii_lowercase();
    let latin1 = charset.contains("8859-1")
        || charset.contains("latin1")
        || charset.contains("latin-1")
        || charset.contains("windows-1252");
    if latin1 {
        let mut out = String::with_capacity(body.len());
        for &byte in body {
            out.push(byte as char);
        }
        out
    } else {
        String::from_utf8_lossy(body).into_owned()
    }
}

/// Build the request bytes for a `GET`, ready to be written to a TCP socket.
pub fn request_bytes(url: &Url) -> String {
    request_bytes_with(url, &[])
}

/// Build a `GET` request with additional headers (`(name, value)` pairs).
pub fn request_bytes_with(url: &Url, extra_headers: &[(&str, &str)]) -> String {
    let mut request = String::with_capacity(256);
    request.push_str("GET ");
    request.push_str(&url.request_target());
    request.push_str(" HTTP/1.1\r\nHost: ");
    request.push_str(&url.host_header());
    request.push_str("\r\nUser-Agent: ");
    request.push_str(USER_AGENT);
    request.push_str("\r\nAccept: text/html,application/xhtml+xml,text/plain;q=0.9,*/*;q=0.5");
    request.push_str("\r\nAccept-Encoding: identity");
    for (name, value) in extra_headers {
        request.push_str("\r\n");
        request.push_str(name);
        request.push_str(": ");
        request.push_str(value);
    }
    request.push_str("\r\nConnection: close\r\n\r\n");
    request
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn url() -> Url {
        Url::parse("http://example.com/index.html").expect("url")
    }

    fn drain(parser: &mut ResponseParser, chunks: &[&[u8]]) -> Response {
        for chunk in chunks {
            parser.feed(chunk).expect("feed should succeed");
        }
        let _ = parser.finish();
        parser.response().clone()
    }

    #[test]
    fn decodes_content_length_response_split_across_reads() {
        let mut parser = ResponseParser::new(4096);
        let response = drain(
            &mut parser,
            &[
                b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Le",
                b"ngth: 11\r\n\r\nhello world",
            ],
        );
        assert_eq!(response.status, 200);
        assert_eq!(response.reason, "OK");
        assert_eq!(response.body, b"hello world");
        assert!(response.complete);
        assert!(!response.truncated);
        assert_eq!(response.mime(), "text/html");
        assert_eq!(response.status_line(), "HTTP 200 OK");
    }

    #[test]
    fn decodes_chunked_response_byte_by_byte() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let mut parser = ResponseParser::new(4096);
        for byte in raw.iter() {
            parser.feed(core::slice::from_ref(byte)).expect("feed");
        }
        assert!(parser.is_complete());
        assert_eq!(parser.response().body, b"hello world");
        assert!(!parser.response().truncated);
    }

    #[test]
    fn handles_chunk_extensions_and_trailers() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3;ext=1\r\nabc\r\n0\r\nX-Trailer: 1\r\n\r\n";
        let mut parser = ResponseParser::new(4096);
        parser.feed(raw).expect("feed");
        assert_eq!(parser.response().body, b"abc");
        assert!(parser.is_complete());
    }

    #[test]
    fn reads_until_close_without_length() {
        let mut parser = ResponseParser::new(4096);
        parser.feed(b"HTTP/1.0 200 OK\r\n\r\nbody").expect("feed");
        assert!(!parser.is_complete());
        parser.finish().expect("finish");
        assert!(parser.is_complete());
        assert_eq!(parser.response().body, b"body");
    }

    #[test]
    fn truncates_oversized_bodies() {
        let mut parser = ResponseParser::new(1024);
        parser
            .feed(b"HTTP/1.1 200 OK\r\nContent-Length: 5000\r\n\r\n")
            .expect("head");
        parser.feed(&vec![b'x'; 4096]).expect("body");
        let response = parser.response();
        assert!(response.truncated);
        assert_eq!(response.body.len(), 1024);
    }

    #[test]
    fn marks_short_content_length_as_truncated() {
        let mut parser = ResponseParser::new(4096);
        parser
            .feed(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nshort")
            .expect("feed");
        parser.finish().expect("finish");
        assert!(parser.response().truncated);
    }

    #[test]
    fn rejects_malformed_responses() {
        let mut parser = ResponseParser::new(4096);
        assert_eq!(
            parser.feed(b"ICY 200 OK\r\n\r\n"),
            Err(HttpError::UnsupportedVersion)
        );

        let mut parser = ResponseParser::new(4096);
        assert_eq!(
            parser.feed(b"HTTP/1.1 20 OK\r\n\r\n"),
            Err(HttpError::MalformedStatusLine)
        );

        let mut parser = ResponseParser::new(4096);
        assert!(parser
            .feed(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n")
            .is_err());
    }

    #[test]
    fn header_lookup_is_case_insensitive() {
        let mut parser = ResponseParser::new(4096);
        parser
            .feed(b"HTTP/1.1 302 Found\r\nLocation: /next\r\nContent-Type: Text/HTML; charset=ISO-8859-1\r\n\r\n")
            .expect("feed");
        let response = parser.response();
        assert!(response.is_redirect());
        assert_eq!(response.location(), Some("/next"));
        assert_eq!(response.mime(), "text/html");
        assert_eq!(response.charset(), "iso-8859-1");
        assert_eq!(response.unsupported_encoding(), None);
    }

    #[test]
    fn detects_unsupported_content_encoding() {
        let mut parser = ResponseParser::new(4096);
        parser
            .feed(b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\n")
            .expect("feed");
        assert_eq!(parser.response().unsupported_encoding(), Some("gzip"));
    }

    #[test]
    fn bodyless_statuses_complete_immediately() {
        let mut parser = ResponseParser::new(4096);
        parser
            .feed(b"HTTP/1.1 204 No Content\r\n\r\n")
            .expect("feed");
        assert!(parser.is_complete());
        assert!(parser.response().body.is_empty());
    }

    #[test]
    fn decodes_latin1_when_declared() {
        let text = decode_text(&[0xE4, 0xF6, 0xFC], "iso-8859-1");
        assert_eq!(text, "äöü");
        assert_eq!(decode_text(b"ok", "utf-8"), "ok");
        assert!(decode_text(&[0xFF, b'a'], "utf-8").ends_with('a'));
    }

    #[test]
    fn builds_get_request() {
        let request = request_bytes(&url());
        assert!(request.starts_with("GET /index.html HTTP/1.1\r\n"));
        assert!(request.contains("Host: example.com\r\n"));
        assert!(request.contains("Accept-Encoding: identity"));
        assert!(request.ends_with("\r\n\r\n"));
        let with_headers = request_bytes_with(&url(), &[("X-Test", "1")]);
        assert!(with_headers.contains("X-Test: 1\r\n"));
    }

    #[test]
    fn huge_header_block_is_rejected() {
        let mut parser = ResponseParser::new(4096);
        let mut junk = Vec::from(b"HTTP/1.1 200 OK\r\n".as_slice());
        for _ in 0..2000 {
            junk.extend_from_slice(b"X-Padding: 0123456789\r\n");
        }
        junk.extend_from_slice(b"\r\n");
        assert_eq!(parser.feed(&junk), Err(HttpError::HeadersTooLarge));
    }

    #[test]
    fn finish_before_headers_is_an_error() {
        let mut parser = ResponseParser::new(4096);
        parser.feed(b"HTTP/1.1 200").expect("feed");
        assert_eq!(parser.finish(), Err(HttpError::IncompleteHead));
    }
}
