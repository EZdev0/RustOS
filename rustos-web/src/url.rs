//! HTTP/1.1 `GET` URL parsing and relative-reference resolution.
//!
//! The kernel ships without TLS and without a DNS resolver, so this module
//! deliberately models exactly what the on-disk image can do:
//!
//! * `http://` only (`https://` is reported as an explicit, user visible error
//!   instead of silently producing a broken request),
//! * host names are kept verbatim (lower-cased) — turning a name into an IPv4
//!   address is the caller's job (`Url::ipv4` handles literals, the browser app
//!   owns a tiny static hosts table),
//! * every parser path is allocation-bounded and panic-free.

use alloc::format;
use alloc::string::{String, ToString};

/// Port used when the URL does not spell one out.
pub const DEFAULT_HTTP_PORT: u16 = 80;

/// Refuse absurdly long URLs early; the URL bar cannot produce more anyway.
pub const MAX_URL_LEN: usize = 2048;

/// A parsed `http://` URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    scheme: String,
    host: String,
    port: u16,
    path: String,
    query: Option<String>,
}

/// Everything that can go wrong while parsing a URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlError {
    /// The input was empty or only whitespace.
    Empty,
    /// The input exceeded [`MAX_URL_LEN`].
    TooLong,
    /// A scheme other than `http` was requested (including `https`).
    UnsupportedScheme,
    /// No host component, e.g. `http:///path`.
    MissingHost,
    /// The port was not a decimal number in `1..=65535`.
    InvalidPort,
    /// The host contained characters that are not valid in a host name.
    InvalidHost,
}

impl UrlError {
    /// Human readable explanation, shown directly in the browser status bar.
    pub fn message(self) -> &'static str {
        match self {
            UrlError::Empty => "empty address",
            UrlError::TooLong => "address too long",
            UrlError::UnsupportedScheme => "only http:// is supported (no TLS yet)",
            UrlError::MissingHost => "missing host name",
            UrlError::InvalidPort => "invalid port",
            UrlError::InvalidHost => "invalid host name",
        }
    }
}

impl Url {
    /// Parse an absolute URL, tolerating a missing scheme (`example.com/x`
    /// becomes `http://example.com/x`).
    pub fn parse(input: &str) -> Result<Self, UrlError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(UrlError::Empty);
        }
        if trimmed.len() > MAX_URL_LEN {
            return Err(UrlError::TooLong);
        }

        // Fragments never reach the network.
        let without_fragment = match trimmed.find('#') {
            Some(index) => &trimmed[..index],
            None => trimmed,
        };

        let (scheme, rest) = match without_fragment.find("://") {
            Some(index) => (
                without_fragment[..index].to_ascii_lowercase(),
                &without_fragment[index + 3..],
            ),
            None => match without_fragment.strip_prefix("//") {
                Some(rest) => ("http".to_string(), rest),
                None => ("http".to_string(), without_fragment),
            },
        };

        if scheme != "http" {
            return Err(UrlError::UnsupportedScheme);
        }

        let authority_end = without_fragment_end(rest);
        let authority = &rest[..authority_end];
        let tail = &rest[authority_end..];

        let host_port = match authority.rfind('@') {
            Some(index) => &authority[index + 1..],
            None => authority,
        };
        if host_port.is_empty() {
            return Err(UrlError::MissingHost);
        }

        let (host, port) = split_host_port(host_port)?;
        if host.is_empty() {
            return Err(UrlError::MissingHost);
        }
        if !is_valid_host(&host) {
            return Err(UrlError::InvalidHost);
        }

        let (path_part, query) = match tail.find('?') {
            Some(index) => (&tail[..index], Some(tail[index + 1..].to_string())),
            None => (tail, None),
        };

        let path = if path_part.is_empty() {
            "/".to_string()
        } else {
            percent_encode_path(path_part)
        };

        Ok(Self {
            scheme,
            host,
            port,
            path,
            query,
        })
    }

    /// The scheme, always `"http"` for URLs produced by [`Url::parse`].
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    /// The host as written (lower-cased), without brackets for IPv6 literals.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The effective port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The request path, always starting with `/`.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The query string without the leading `?`, if any.
    pub fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    /// Value for the HTTP `Host:` header (port omitted when it is 80).
    ///
    /// IPv6 literals must keep their brackets: `Host: [::1]:8080`, not
    /// `Host: ::1:8080` (the latter is ambiguous and rejected by servers).
    pub fn host_header(&self) -> String {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        if self.port == DEFAULT_HTTP_PORT {
            host
        } else {
            format!("{}:{}", host, self.port)
        }
    }

    /// The request target: path plus optional query string.
    pub fn request_target(&self) -> String {
        match &self.query {
            Some(query) => format!("{}?{}", self.path, query),
            None => self.path.clone(),
        }
    }

    /// Canonical `scheme://host[:port]/path[?query]` form for the URL bar.
    pub fn display(&self) -> String {
        let mut out = format!("{}://{}", self.scheme, self.host_header());
        out.push_str(&self.path);
        if let Some(query) = &self.query {
            out.push('?');
            out.push_str(query);
        }
        out
    }

    /// `scheme://host:port` key that identifies a document origin.
    pub fn origin_key(&self) -> String {
        format!("{}://{}:{}", self.scheme, self.host, self.port)
    }

    /// True when this URL can be fetched without a DNS lookup.
    pub fn is_ip_literal(&self) -> bool {
        self.ipv4().is_some()
    }

    /// Parse the host as a dotted-quad IPv4 literal, if it is one.
    ///
    /// This is a hand written parser (no `std::net`) so the kernel can use it.
    pub fn ipv4(&self) -> Option<[u8; 4]> {
        parse_ipv4(&self.host)
    }

    /// Resolve a relative reference against this URL (RFC 3986 §5.2 in
    /// miniature, without the exotic cases this engine never needs).
    pub fn resolve(&self, reference: &str) -> Result<Self, UrlError> {
        let reference = reference.trim();
        if reference.is_empty() {
            return Ok(self.clone());
        }
        if reference.starts_with('#') {
            return Ok(self.clone());
        }
        if reference.contains("://") {
            return Url::parse(reference);
        }
        if let Some(scheme_relative) = reference.strip_prefix("//") {
            return Url::parse(&format!("{}://{}", self.scheme, scheme_relative));
        }
        if reference.starts_with('/') {
            let mut resolved = self.clone();
            let (path, query) = split_path_query(reference);
            resolved.path = if path.is_empty() {
                "/".to_string()
            } else {
                percent_encode_path(path)
            };
            resolved.query = query;
            return Ok(resolved);
        }
        if let Some(query) = reference.strip_prefix('?') {
            let mut resolved = self.clone();
            resolved.query = Some(query.to_string());
            return Ok(resolved);
        }

        let directory = match self.path.rfind('/') {
            Some(index) => &self.path[..=index],
            None => "/",
        };
        let (rel_path, query) = split_path_query(reference);
        let combined = format!("{}{}", directory, rel_path);
        let mut resolved = self.clone();
        resolved.path = normalize_path(&combined);
        resolved.query = query;
        Ok(resolved)
    }

    /// Clone this URL with a different path (used by redirect handling).
    pub fn with_path(&self, path: &str) -> Self {
        let (clean_path, query) = split_path_query(path);
        let mut next = self.clone();
        next.path = if clean_path.is_empty() {
            "/".to_string()
        } else {
            percent_encode_path(clean_path)
        };
        if query.is_some() {
            next.query = query;
        }
        next
    }
}

fn without_fragment_end(rest: &str) -> usize {
    rest.find(['/', '?']).unwrap_or(rest.len())
}

fn split_path_query(input: &str) -> (&str, Option<String>) {
    match input.find('?') {
        Some(index) => (&input[..index], Some(input[index + 1..].to_string())),
        None => (input, None),
    }
}

fn split_host_port(host_port: &str) -> Result<(String, u16), UrlError> {
    if let Some(rest) = host_port.strip_prefix('[') {
        // IPv6 literal: [::1]:8080
        let close = rest.find(']').ok_or(UrlError::InvalidHost)?;
        let host = &rest[..close];
        let after = &rest[close + 1..];
        let port = if after.is_empty() {
            DEFAULT_HTTP_PORT
        } else if let Some(port_str) = after.strip_prefix(':') {
            parse_port(port_str)?
        } else {
            return Err(UrlError::InvalidHost);
        };
        return Ok((host.to_ascii_lowercase(), port));
    }

    match host_port.rfind(':') {
        Some(index) => {
            let host = &host_port[..index];
            let port_str = &host_port[index + 1..];
            let port = if port_str.is_empty() {
                DEFAULT_HTTP_PORT
            } else {
                parse_port(port_str)?
            };
            Ok((host.to_ascii_lowercase(), port))
        }
        None => Ok((host_port.to_ascii_lowercase(), DEFAULT_HTTP_PORT)),
    }
}

fn parse_port(input: &str) -> Result<u16, UrlError> {
    if input.is_empty() || input.len() > 5 || !input.bytes().all(|b| b.is_ascii_digit()) {
        return Err(UrlError::InvalidPort);
    }
    let value: u32 = input.parse().map_err(|_| UrlError::InvalidPort)?;
    if value == 0 || value > 65535 {
        return Err(UrlError::InvalidPort);
    }
    Ok(value as u16)
}

fn is_valid_host(host: &str) -> bool {
    if host.len() > 253 || host.is_empty() {
        return false;
    }
    // IPv6 literal.
    if host.contains(':') {
        return host
            .bytes()
            .all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.');
    }
    if host.starts_with('.') || host.ends_with('.') || host.contains("..") {
        return false;
    }
    host.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_')
}

fn parse_ipv4(host: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut count = 0;
    for part in host.split('.') {
        if count >= 4 || part.is_empty() || part.len() > 3 {
            return None;
        }
        if !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let value: u32 = part.parse().ok()?;
        if value > 255 {
            return None;
        }
        octets[count] = value as u8;
        count += 1;
    }
    if count == 4 {
        Some(octets)
    } else {
        None
    }
}

/// Percent-encode everything that is not allowed in an HTTP request target.
fn percent_encode_path(path: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        let safe = byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'.'
                    | b'_'
                    | b'~'
                    | b'/'
                    | b'?'
                    | b'&'
                    | b'='
                    | b'+'
                    | b'%'
                    | b':'
                    | b'@'
                    | b'!'
                    | b'$'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b','
                    | b';'
                    | b'['
                    | b']'
            );
        if safe {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0F) as usize] as char);
        }
    }
    out
}

/// Collapse `.`/`..` segments and duplicate slashes.
fn normalize_path(path: &str) -> String {
    // A directory reference ("g/", ".", "..") keeps its trailing slash.
    let trailing = path.ends_with('/') || path.ends_with("/.") || path.ends_with("/..");
    let mut segments: alloc::vec::Vec<&str> = alloc::vec::Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    let mut out = String::from("/");
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            out.push('/');
        }
        out.push_str(segment);
    }
    if trailing && !out.ends_with('/') {
        out.push('/');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(input: &str) -> Url {
        Url::parse(input).expect("url should parse")
    }

    #[test]
    fn parses_simple_absolute_url() {
        let parsed = url("http://example.com/hello?q=1");
        assert_eq!(parsed.scheme(), "http");
        assert_eq!(parsed.host(), "example.com");
        assert_eq!(parsed.port(), 80);
        assert_eq!(parsed.path(), "/hello");
        assert_eq!(parsed.query(), Some("q=1"));
        assert_eq!(parsed.request_target(), "/hello?q=1");
        assert_eq!(parsed.host_header(), "example.com");
    }

    #[test]
    fn adds_default_path_and_scheme() {
        assert_eq!(url("example.com").path(), "/");
        assert_eq!(url("EXAMPLE.com/A").host_header(), "example.com");
        assert_eq!(url("//example.com/x").display(), "http://example.com/x");
    }

    #[test]
    fn handles_ports_userinfo_and_ipv6() {
        let with_port = url("http://example.com:8080/");
        assert_eq!(with_port.port(), 8080);
        assert_eq!(with_port.host_header(), "example.com:8080");
        assert_eq!(url("http://user:pw@example.com/").host(), "example.com");
        let v6 = url("http://[::1]:81/");
        assert_eq!(v6.host(), "::1");
        assert_eq!(v6.port(), 81);
    }

    #[test]
    fn encodes_unsafe_characters() {
        assert_eq!(url("http://example.com/a b").path(), "/a%20b");
        assert_eq!(url("http://example.com/ä").path(), "/%C3%A4");
    }

    #[test]
    fn rejects_broken_urls() {
        assert_eq!(Url::parse(""), Err(UrlError::Empty));
        assert_eq!(Url::parse("   "), Err(UrlError::Empty));
        assert_eq!(
            Url::parse("https://example.com/"),
            Err(UrlError::UnsupportedScheme)
        );
        assert_eq!(Url::parse("http:///path"), Err(UrlError::MissingHost));
        assert_eq!(
            Url::parse("http://example.com:0/"),
            Err(UrlError::InvalidPort)
        );
        assert_eq!(
            Url::parse("http://example.com:99999/"),
            Err(UrlError::InvalidPort)
        );
        assert_eq!(
            Url::parse("http://exa mple.com/"),
            Err(UrlError::InvalidHost)
        );
        let long = alloc::format!("http://{}.com/", "a".repeat(MAX_URL_LEN));
        assert_eq!(Url::parse(&long), Err(UrlError::TooLong));
    }

    #[test]
    fn resolves_relative_references() {
        let base = url("http://example.com/docs/guide/index.html");
        assert_eq!(
            base.resolve("page2.html").unwrap().path(),
            "/docs/guide/page2.html"
        );
        assert_eq!(
            base.resolve("../top.html").unwrap().path(),
            "/docs/top.html"
        );
        assert_eq!(base.resolve("/root.html").unwrap().path(), "/root.html");
        assert_eq!(base.resolve("?a=1").unwrap().query(), Some("a=1"));
        assert_eq!(base.resolve("#frag").unwrap(), base);
        assert_eq!(
            base.resolve("//other.example/x").unwrap().host(),
            "other.example"
        );
        assert_eq!(
            base.resolve("http://third.example/").unwrap().host(),
            "third.example"
        );
        assert_eq!(base.resolve("").unwrap(), base);
    }

    #[test]
    fn detects_ipv4_literals() {
        let ip = url("http://93.184.216.34/");
        assert!(ip.is_ip_literal());
        assert_eq!(ip.ipv4(), Some([93, 184, 216, 34]));
        assert!(!url("http://example.com/").is_ip_literal());
        assert_eq!(url("http://1.2.3.4.5/").ipv4(), None);
        assert_eq!(url("http://256.1.1.1/").ipv4(), None);
        assert_eq!(url("http://x").ipv4(), None);
    }

    #[test]
    fn path_replacement_keeps_origin() {
        let redirect = url("http://example.com/old").with_path("/new?a=b");
        assert_eq!(redirect.display(), "http://example.com/new?a=b");
        assert_eq!(redirect.origin_key(), "http://example.com:80");
    }

    #[test]
    fn normalize_path_handles_dot_segments() {
        assert_eq!(normalize_path("/a/./b/../c"), "/a/c");
        assert_eq!(normalize_path("/../.."), "/");
        assert_eq!(normalize_path("/"), "/");
    }
}
