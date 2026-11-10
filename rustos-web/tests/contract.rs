//! End-to-end contract tests for the public API of `rustos-web`.
//!
//! These complement the unit tests inside the crate: they only use what the
//! kernel is allowed to use, so they also prove that the public surface is
//! sufficient (and safe) for the browser app.

use rustos_web::engine::{PageEngine, MAX_REDIRECTS};
use rustos_web::html::Document;
use rustos_web::http::{HttpError, ResponseParser};
use rustos_web::layout::{layout_document, DrawCmd, LayoutOptions};
use rustos_web::url::Url;

const WIDTH: u32 = 640;

fn opts(width: u32) -> LayoutOptions {
    LayoutOptions {
        width,
        dark_mode: false,
    }
}

fn layout(html: &str) -> rustos_web::layout::Layout {
    layout_document(&Document::parse(html), opts(WIDTH))
}

fn texts(layout: &rustos_web::layout::Layout) -> Vec<String> {
    layout
        .cmds
        .iter()
        .filter_map(|cmd| match cmd {
            DrawCmd::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn all_text(layout: &rustos_web::layout::Layout) -> String {
    texts(layout).join(" ")
}

// ─── URL ────────────────────────────────────────────────────────────────────

#[test]
fn url_is_case_insensitive_for_scheme_and_host() {
    let url = Url::parse("HTTP://Example.COM/Path").expect("uppercase url");
    assert_eq!(url.scheme(), "http");
    assert_eq!(url.host(), "example.com");
    assert_eq!(url.host_header(), "example.com");
    assert_eq!(url.request_target(), "/Path");
}

#[test]
fn url_keeps_port_in_host_header_only() {
    let url = Url::parse("http://example.com:8080/a?b=1#frag").expect("url");
    assert_eq!(url.host(), "example.com");
    assert_eq!(url.host_header(), "example.com:8080");
    assert_eq!(url.request_target(), "/a?b=1");
}

#[test]
fn url_rejects_non_http_and_broken_input() {
    for broken in [
        "https://example.com/",
        "ftp://example.com/",
        "http://",
        "http:///path",
        "http://exa mple.com/",
        "http://example.com:99999/",
    ] {
        assert!(Url::parse(broken).is_err(), "{broken} should be rejected");
    }
    // A bare host is a convenience for the URL bar, not an error.
    let bare = Url::parse("example.com/x").expect("bare host");
    assert_eq!(bare.display(), "http://example.com/x");
}

#[test]
fn url_resolution_matches_rfc_examples() {
    let base = Url::parse("http://a/b/c/d;p?q").expect("base");
    let cases = [
        ("g", "http://a/b/c/g"),
        ("./g", "http://a/b/c/g"),
        ("g/", "http://a/b/c/g/"),
        ("/g", "http://a/g"),
        ("//g", "http://g/"),
        ("?y", "http://a/b/c/d;p?y"),
        ("../g", "http://a/b/g"),
        ("../../g", "http://a/g"),
    ];
    for (reference, expected) in cases {
        let resolved = base.resolve(reference).expect(reference);
        assert_eq!(resolved.display(), expected, "resolving {reference}");
    }
}

#[test]
fn url_keeps_ipv6_host_bracketed() {
    let url = Url::parse("http://[::1]:8080/x").expect("ipv6 url");
    assert_eq!(url.host_header(), "[::1]:8080");
}

// ─── HTTP ───────────────────────────────────────────────────────────────────

fn parse_chunked(chunks: &[&[u8]], cap: usize) -> rustos_web::http::Response {
    let mut parser = ResponseParser::new(cap);
    for chunk in chunks {
        parser.feed(chunk).expect("feed");
    }
    let _ = parser.finish();
    parser.response().clone()
}

const CHUNKED: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nTransfer-Encoding: chunked\r\n\r\n\
5\r\nhello\r\n6;ext=1\r\n world\r\n0\r\nX-Trailer: 1\r\n\r\n";

#[test]
fn chunked_body_is_identical_byte_by_byte() {
    let one_shot = parse_chunked(&[CHUNKED], 4096);
    assert_eq!(one_shot.body, b"hello world");
    assert!(one_shot.complete);
    assert!(!one_shot.truncated);

    // Every possible split point must produce the same result.
    for split in 0..CHUNKED.len() {
        let (a, b) = CHUNKED.split_at(split);
        let response = parse_chunked(&[a, b], 4096);
        assert_eq!(response.body, one_shot.body, "split at {split}");
        assert!(response.complete, "split at {split}");
    }

    // And byte by byte.
    let chunks: Vec<&[u8]> = CHUNKED.chunks(1).collect();
    let response = parse_chunked(&chunks, 4096);
    assert_eq!(response.body, b"hello world");
}

#[test]
fn content_length_framing_ignores_extra_bytes() {
    let response = parse_chunked(
        &[b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhelloEXTRA"],
        4096,
    );
    assert_eq!(response.body, b"hello");
    assert!(response.complete);
}

#[test]
fn zero_length_body_completes_immediately() {
    let response = parse_chunked(
        &[b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n"],
        64,
    );
    assert!(response.complete);
    assert!(response.body.is_empty());
    assert_eq!(response.status, 204);
}

#[test]
fn body_budget_truncates_instead_of_growing() {
    let mut parser = ResponseParser::new(2048);
    parser
        .feed(b"HTTP/1.1 200 OK\r\nContent-Length: 100000\r\n\r\n")
        .expect("head");
    for _ in 0..100 {
        let _ = parser.feed(&[b'x'; 1000]);
    }
    let response = parser.response().clone();
    assert!(response.truncated);
    assert!(response.body.len() <= 2048);
    assert!(response.complete);
    let _ = parser.feed(b"more");
    assert!(parser.error().is_none(), "truncation is not an error");
}

#[test]
fn headers_are_case_insensitive_and_ordered() {
    let response = parse_chunked(
        &[b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=UTF-8\r\nX-A: 1\r\nx-a: 2\r\nContent-Length: 0\r\n\r\n"],
        64,
    );
    assert_eq!(response.header("X-A"), Some("1"));
    assert_eq!(response.header("x-a"), Some("1"));
    assert_eq!(response.headers.len(), 4);
    assert_eq!(response.charset(), "utf-8");
    assert_eq!(response.mime(), "text/html");
}

#[test]
fn unsupported_content_encoding_is_reported() {
    let response = parse_chunked(
        &[b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 2\r\n\r\n\x1f\x8b"],
        64,
    );
    assert_eq!(response.unsupported_encoding(), Some("gzip"));
    let identity = parse_chunked(
        &[b"HTTP/1.1 200 OK\r\nContent-Encoding: identity\r\nContent-Length: 0\r\n\r\n"],
        64,
    );
    assert_eq!(identity.unsupported_encoding(), None);
}

#[test]
fn malformed_responses_are_errors_not_panics() {
    let mut parser = ResponseParser::new(64);
    assert_eq!(
        parser.feed(b"ICY 200 OK\r\n\r\n"),
        Err(HttpError::UnsupportedVersion)
    );

    let mut parser = ResponseParser::new(64);
    assert_eq!(
        parser.feed(b"HTTP/1.1 20 OK\r\n\r\n"),
        Err(HttpError::MalformedStatusLine)
    );

    let mut parser = ResponseParser::new(64);
    assert_eq!(
        parser.feed(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nabcdZZZ"),
        Err(HttpError::BadChunk)
    );

    let mut parser = ResponseParser::new(64);
    assert_eq!(parser.finish(), Err(HttpError::IncompleteHead));
}

#[test]
fn oversized_headers_are_rejected() {
    let mut parser = ResponseParser::new(64);
    let head = format!("HTTP/1.1 200 OK\r\nX: {}\r\n\r\n", "a".repeat(20_000));
    assert_eq!(
        parser.feed(head.as_bytes()),
        Err(HttpError::HeadersTooLarge)
    );
}

#[test]
fn closed_connection_marks_truncation() {
    let mut parser = ResponseParser::new(4096);
    parser
        .feed(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\npartial")
        .expect("feed");
    parser.finish().expect("finish");
    assert!(parser.response().truncated);
    assert!(parser.response().complete);
    assert_eq!(parser.response().body, b"partial");
}

// ─── HTML ───────────────────────────────────────────────────────────────────

#[test]
fn titles_and_entities_are_decoded() {
    let document = Document::parse(
        "<html><head><title>Tom &amp; Jerry &lt;3</title></head><body>&#65;&#x42;&nbsp;C</body></html>",
    );
    assert_eq!(document.title.clone().unwrap_or_default(), "Tom & Jerry <3");
    let text: String = document.root.text_content();
    assert!(text.contains("AB C"), "got {text:?}");
}

#[test]
fn script_content_never_leaks() {
    let document = Document::parse(
        "<p>before</p><script>if (a < b) { document.write('<p>x</p>'); }</script><p>after</p>",
    );
    let text = document.root.text_content();
    assert!(text.contains("before"));
    assert!(text.contains("after"));
    assert!(!text.contains("document.write"), "script leaked: {text:?}");
}

#[test]
fn hostile_markup_does_not_panic() {
    let cases = [
        "<div",
        "<div class=",
        "<<<>>>",
        "</>",
        "<a href='unterminated",
        "<!-- unterminated comment",
        "<!-->",
        "<p>\u{0}\u{1}\u{2}</p>",
        "<table><tr><td>a<td>b</table>",
        "<p>1<p>2<p>3",
        &format!("<div data-x=\"{}\">deep</div>", "y".repeat(10_000)),
        &"<div>".repeat(500),
        &format!("{}text{}", "<span>".repeat(200), "</span>".repeat(100)),
    ];
    for case in cases {
        let layout = layout(case);
        assert!(layout.height > 0, "no content for {case:?}");
    }
}

#[test]
fn deeply_nested_markup_is_bounded() {
    let html = format!("{}deep{}", "<div>".repeat(5_000), "</div>".repeat(5_000));
    let document = Document::parse(&html);
    assert!(document.root.text_content().contains("deep"));
    // The parser must refuse to build an unbounded tree (depth cap).
    fn count(node: &rustos_web::html::Node) -> usize {
        1 + node.children.iter().map(count).sum::<usize>()
    }
    assert!(count(&document.root) <= 20_000);
}

// ─── CSS ────────────────────────────────────────────────────────────────────

fn color_of_first_text(layout: &rustos_web::layout::Layout) -> rustos_web::Color {
    layout
        .cmds
        .iter()
        .find_map(|cmd| match cmd {
            DrawCmd::Text { color, .. } => Some(*color),
            _ => None,
        })
        .expect("text command")
}

#[test]
fn cascade_order_is_id_class_tag_inline() {
    let html = "\
<style>
p { color: #111111; }
.cls { color: #222222; }
#id { color: #333333; }
</style>
<p class=\"cls\" id=\"id\">x</p>\
";
    assert_eq!(
        color_of_first_text(&layout(html)),
        rustos_web::Color::rgb(0x33, 0x33, 0x33)
    );

    let inline = "\
<style>p { color: #111111; }</style>
<p style=\"color: #444444\">x</p>\
";
    assert_eq!(
        color_of_first_text(&layout(inline)),
        rustos_web::Color::rgb(0x44, 0x44, 0x44)
    );
}

#[test]
fn display_none_and_bad_declarations_are_harmless() {
    let hidden = layout("<p style=\"display:none\">invisible</p><p>visible</p>");
    let text = all_text(&hidden);
    assert!(!text.contains("invisible"), "got {text:?}");
    assert!(text.contains("visible"));

    let broken = layout("<p style=\"color:;font-size:;;;display\">ok</p>");
    assert!(all_text(&broken).contains("ok"));
}

#[test]
fn huge_font_sizes_are_clamped() {
    let layout = layout("<p style=\"font-size:9999px\">big</p>");
    for cmd in &layout.cmds {
        if let DrawCmd::Text { scale, .. } = cmd {
            assert!(*scale <= 4, "scale {scale} not clamped");
        }
    }
}

// ─── Layout ─────────────────────────────────────────────────────────────────

#[test]
fn block_commands_stay_inside_the_viewport() {
    let layout = layout("<div><p>hello world</p><h1>title</h1></div>");
    for cmd in &layout.cmds {
        let (x, y, w, _) = cmd.bounds();
        assert!(x >= 0, "negative x in {cmd:?}");
        assert!(y >= 0, "negative y in {cmd:?}");
        assert!(
            (x as u32).saturating_add(w) <= WIDTH,
            "overflowing command {cmd:?}"
        );
    }
}

#[test]
fn long_words_do_not_escape_the_page() {
    let word = "x".repeat(400);
    let layout = layout(&format!("<p>{word}</p>"));
    for cmd in &layout.cmds {
        let (x, _, w, _) = cmd.bounds();
        assert!(
            (x as u32).saturating_add(w) <= WIDTH,
            "long word overflowed: {cmd:?}"
        );
    }
}

#[test]
fn viewport_culling_includes_partially_visible_lines() {
    let html: String = (0..200).map(|i| format!("<p>line {i}</p>")).collect();
    let layout = layout(&html);
    assert!(layout.height > 100);

    // A 1px band through the middle of the first command must show it.
    let (_, first_y, _, first_h) = layout.cmds.first().expect("commands").bounds();
    let band_top = first_y + first_h as i32 / 2;
    let visible: Vec<&DrawCmd> = layout.cmds_in_view(band_top, 1).collect();
    assert!(
        !visible.is_empty(),
        "nothing visible in a 1px band at {band_top}"
    );

    // Everything visible must intersect the band.
    for cmd in visible {
        let (_, y, _, h) = cmd.bounds();
        assert!(y + h as i32 > band_top && y < band_top + 1);
    }

    // Scrolling to the end still yields content.
    let max = layout.max_scroll(300);
    let bottom: Vec<&DrawCmd> = layout.cmds_in_view(max as i32, 300).collect();
    assert!(!bottom.is_empty());
}

#[test]
fn max_scroll_is_zero_for_short_pages() {
    let layout = layout("<p>short</p>");
    assert_eq!(layout.max_scroll(10_000), 0);
    assert!(layout.max_scroll(1) >= layout.max_scroll(300));
}

#[test]
fn pre_keeps_whitespace_and_does_not_wrap() {
    let layout = layout("<pre>a    b\nc</pre>");
    let text = texts(&layout);
    assert!(text.iter().any(|t| t.contains("a    b")), "got {text:?}");
}

#[test]
fn command_budget_is_enforced_for_absurd_pages() {
    let html: String = (0..30_000)
        .map(|i| format!("<p>paragraph {i}</p>"))
        .collect();
    let layout = layout(&html);
    assert!(layout.cmds.len() <= 20_000);
    assert!(layout.truncated);
}

// ─── Engine ─────────────────────────────────────────────────────────────────

fn engine_from(html: &str) -> PageEngine {
    let url = Url::parse("http://example.com/").expect("url");
    PageEngine::from_html(url, html)
}

#[test]
fn engine_renders_offline_html() {
    let mut engine = engine_from("<h1>Hello</h1><p>world</p>");
    assert!(!engine.status_text().is_empty());
    let layout = engine.layout(opts(WIDTH)).expect("layout");
    assert!(all_text(layout).contains("Hello"));
    // Second call must reuse the cached layout (same width + theme).
    let first_len = layout.cmds.len();
    let again_len = engine
        .layout(opts(WIDTH))
        .expect("cached layout")
        .cmds
        .len();
    assert_eq!(again_len, first_len);
}

#[test]
fn engine_feeds_chunked_response_byte_by_byte() {
    let url = Url::parse("http://example.com/").expect("url");
    let mut engine = PageEngine::start(url);
    assert!(engine.take_request().is_some(), "no request produced");
    assert!(engine.take_request().is_none(), "request sent twice");

    let body = "<h1>chunked</h1><p>delivery</p>";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    for byte in response.as_bytes() {
        engine.feed(&[*byte]);
    }
    engine.finish_stream();
    let layout = engine.layout(opts(WIDTH)).expect("layout");
    assert!(all_text(layout).contains("chunked"));
}

#[test]
fn engine_follows_redirects_and_bounds_loops() {
    let url = Url::parse("http://example.com/start").expect("url");
    let mut engine = PageEngine::start(url);
    let _ = engine.take_request();

    let mut hops = 0;
    loop {
        engine.feed(b"HTTP/1.1 302 Found\r\nLocation: /next\r\nContent-Length: 0\r\n\r\n");
        engine.finish_stream();
        hops += 1;
        match engine.take_request() {
            Some(_) => {
                assert!(
                    hops <= MAX_REDIRECTS as usize + 1,
                    "redirect loop not bounded"
                );
                if hops > MAX_REDIRECTS as usize {
                    break;
                }
            }
            None => break,
        }
    }
    assert!(hops >= 2, "redirects were not followed");
    // After giving up, the engine must show *something*, not hang forever.
    let layout = engine.layout(opts(WIDTH));
    assert!(layout.is_some() || engine.error_message().is_some());
}

#[test]
fn engine_renders_plain_text_and_rejects_binaries() {
    let url = Url::parse("http://example.com/a.txt").expect("url");

    let mut text_engine = PageEngine::start(url.clone());
    let _ = text_engine.take_request();
    text_engine.feed(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 11\r\n\r\nhello world",
    );
    text_engine.finish_stream();
    let layout = text_engine.layout(opts(WIDTH)).expect("layout");
    assert!(all_text(layout).contains("hello world"));

    let mut image_engine = PageEngine::start(url);
    let _ = image_engine.take_request();
    image_engine
        .feed(b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 4\r\n\r\n\x89PNG");
    image_engine.finish_stream();
    let layout = image_engine.layout(opts(WIDTH)).expect("layout");
    assert!(
        all_text(layout).to_lowercase().contains("content type"),
        "binary content should show an error page"
    );
}

#[test]
fn engine_reports_already_disabled_bodies() {
    let url = Url::parse("http://example.com/").expect("url");
    let mut engine = PageEngine::start(url);
    let _ = engine.take_request();
    engine.feed(b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 2\r\n\r\n\x1f\x8b");
    engine.finish_stream();
    let layout = engine.layout(opts(WIDTH)).expect("layout");
    let text = all_text(layout).to_lowercase();
    assert!(text.contains("encoding"), "got {text:?}");
}

// ─── Tables ─────────────────────────────────────────────────────────────────

#[test]
fn table_cells_of_one_row_share_a_baseline() {
    let page = layout(
        "<table><tr><th>name</th><th>value</th></tr>\
         <tr><td>alpha</td><td>1</td></tr></table>",
    );
    let cells: Vec<(i32, i32, String)> = page
        .cmds
        .iter()
        .filter_map(|cmd| match cmd {
            DrawCmd::Text { x, y, text, .. } => Some((*x, *y, text.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(cells.len(), 4, "expected four cell runs, got {cells:?}");

    // Same row: same baseline, different column.
    let header: Vec<&(i32, i32, String)> = cells.iter().filter(|c| c.2 == "name").collect();
    let value: Vec<&(i32, i32, String)> = cells.iter().filter(|c| c.2 == "value").collect();
    assert_eq!(header.len(), 1);
    assert_eq!(value.len(), 1);
    assert_eq!(header[0].1, value[0].1, "cells of one row must share y");
    assert!(
        value[0].0 > header[0].0,
        "the second column must be right of the first"
    );

    // Second row is below the first.
    let alpha = cells.iter().find(|c| c.2 == "alpha").expect("alpha cell");
    assert!(alpha.1 > header[0].1, "row two must be below row one");
}

#[test]
fn table_stays_inside_the_page_and_is_not_truncated() {
    let mut html = String::from("<table>");
    for row in 0..40 {
        html.push_str("<tr>");
        for column in 0..4 {
            html.push_str(&format!("<td>r{row}c{column}</td>"));
        }
        html.push_str("</tr>");
    }
    html.push_str("</table>");
    let page = layout(&html);
    assert!(!page.truncated, "40x4 cells must fit the command budget");
    for cmd in &page.cmds {
        let (x, y, w, _h) = cmd.bounds();
        assert!(x >= 0, "negative x: {cmd:?}");
        assert!(y >= 0, "negative y: {cmd:?}");
        assert!(x as u32 + w <= page.width, "escapes the page: {cmd:?}");
    }
    assert_eq!(
        all_text(&page).matches("r39c3").count(),
        1,
        "last cell is rendered exactly once"
    );
}

#[test]
fn nested_tables_terminate() {
    let page = layout(
        "<table><tr><td>outer<table><tr><td>inner<table><tr><td>deep</td></tr></table>\
         </td></tr></table></td><td>side</td></tr></table>",
    );
    let text = all_text(&page);
    assert!(text.contains("deep"), "nested content lost: {text}");
    assert!(text.contains("side"), "sibling cell lost: {text}");
}
