//! Render a page and print the resulting display list as plain text.
//!
//! `tools/webprobe.py` compiles this example with plain `rustc` (no Cargo, no
//! network) and checks the output for layout invariants: nothing painted
//! outside the page, no zero-sized commands, sane page height. That gives the
//! engine a testable seam from Python while the kernel stays `no_std`.
//!
//! ```text
//! render_dump html page.html 640      # parse HTML and dump the layout
//! render_dump http response.bin 640   # feed a raw HTTP/1.1 response
//! render_dump html - 640 < page.html  # read stdin
//! ```
//!
//! Output format (one record per line, space separated, stable for the probe):
//!
//! ```text
//! meta title <title>
//! meta status <status line>
//! meta width <px>
//! meta height <px>
//! meta truncated <bool>
//! cmd fill <x> <y> <w> <h>
//! cmd text <x> <y> <w> <h> <scale> <text...>
//! ```

use std::io::Read;

use rustos_web::{DrawCmd, Layout, LayoutOptions, PageEngine, Url};

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| String::from("html"));
    let path = args.next().unwrap_or_else(|| String::from("-"));
    let width: u32 = args
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(640)
        .clamp(64, 4096);

    let mut raw = Vec::new();
    if path == "-" {
        if std::io::stdin().read_to_end(&mut raw).is_err() {
            eprintln!("render_dump: cannot read stdin");
            std::process::exit(2);
        }
    } else if let Ok(bytes) = std::fs::read(&path) {
        raw = bytes;
    } else {
        eprintln!("render_dump: cannot read {path}");
        std::process::exit(2);
    }

    let url = match Url::parse("http://example.com/index.html") {
        Ok(url) => url,
        Err(_) => std::process::exit(2),
    };

    let mut engine = if mode == "http" {
        let mut engine = PageEngine::start(url);
        let _ = engine.take_request();
        engine.feed(&raw);
        engine.finish_stream();
        engine
    } else {
        PageEngine::from_html(url, &String::from_utf8_lossy(&raw))
    };

    let options = LayoutOptions {
        width,
        dark_mode: false,
    };
    let (height, truncated) = match engine.layout(options) {
        Some(layout) => (layout.height, layout.truncated),
        None => (0, true),
    };

    println!("meta title {}", one_line(&engine.title()));
    println!("meta status {}", one_line(&engine.status_text()));
    println!("meta width {width}");
    println!("meta height {height}");
    println!("meta truncated {truncated}");

    if let Some(layout) = engine.layout(options) {
        dump(layout);
    }
}

fn dump(layout: &Layout) {
    for cmd in &layout.cmds {
        let (x, y, w, h) = cmd.bounds();
        match cmd {
            DrawCmd::FillRect { .. } => println!("cmd fill {x} {y} {w} {h}"),
            DrawCmd::Text { text, scale, .. } => {
                println!("cmd text {x} {y} {w} {h} {scale} {}", one_line(text));
            }
        }
    }
}

fn one_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}
