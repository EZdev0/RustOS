# `rustos-web` — the browser engine core

`rustos-web` turns a URL plus a stream of TCP segments into a display list that
the compositor can blit. It is `#![no_std]` (with `alloc`), has **zero
dependencies**, forbids `unsafe`, and runs its unit tests on the host.

```bash
cargo test -p rustos-web --lib   # 83 tests, no QEMU, no bare-metal target
```

---

## Modules

| Module | Responsibility | Key limits |
| --- | --- | --- |
| `url` | parse/normalise `http://` URLs, resolve relative references, `Host:` header | 2048 chars |
| `http` | incremental response decoder (status line, headers, `Content-Length`, chunked, until-close), request encoder, charset decoding | 16 KiB headers, configurable body cap |
| `html` | forgiving tokenizer + tree builder, entity decoding, `<title>`/`<style>` capture | depth 64, 20 000 nodes, iterative parsing |
| `style` | UA stylesheet, `style="…"` declaration parser (integer only) | 512 px lengths |
| `stylesheet` | `<style>` parsing, tag/`.class`/`#id` selectors, specificity order | 256 rules |
| `layout` | block/inline layout, word wrap, lists, tables, `pre`, `hr`, `img` placeholders → `Vec<DrawCmd>` | 20 000 commands, 200 000 px page height |
| `engine` | `PageEngine`: one load = request → chunks → document → cached layout, redirects, status text | 4 redirects, 2 MiB body |

## Supported HTML

Structure: `html head body title meta link style script div span section article
header footer nav main aside h1..h6 p a b strong i em u code pre blockquote ul ol
li dl dt dd table tr td th thead tbody center br hr img input button select
textarea form label`.

* void elements, optional end tags, attribute quoting variants;
* automatic closing of `<p>`, `<li>`, `<td>`, `<tr>`, … so unclosed markup from
  real-world pages still produces a sane tree;
* comments, `doctype`, `<?…>` and a stray `<` never break the parse;
* `<script>`/`<noscript>` content is skipped entirely;
* unknown elements are treated as blocks — content is never dropped.

## Supported CSS

`display`, `color`, `background`/`background-color`, `font-size`,
`font-weight`, `text-align`, `text-decoration`, `margin`, `margin-top`,
`margin-bottom`, `padding`, `padding-left`, `border*`, `white-space`,
`list-style-type`.

Selectors: `tag`, `.class`, `#id`, compounds (`div.notice#main`), comma lists.
Specificity order: tag < class < id; inline `style=` wins over everything.

Not supported (silently ignored, never fatal): `@media` evaluation (blocks are
skipped), pseudo-classes, attribute selectors, combinators (the right-most
compound is used), `!important`, `content`, flexbox/grid, transforms.

## Drawing contract

```rust
pub enum DrawCmd {
    FillRect { x: i32, y: i32, w: u32, h: u32, color: Color },
    Text { x: i32, y: i32, scale: u8, bold: bool, color: Color, text: String },
}
```

* Coordinates are absolute page pixels; the app subtracts the scroll offset.
* Glyph advance is exactly `8 * scale` px, so the compositor can draw text with
  `draw_text(x, y, text, scale, bold, …)` and nothing will drift.
* `bold` means "draw twice, one pixel apart" — the bundled font has no bold face.
* Colors are RGB only; alpha blending stays in the compositor.

## Safety rules enforced in code

* `#![forbid(unsafe_code)]` — no pointer arithmetic in the engine.
* No `unwrap()`/`panic!` outside tests (`clippy::unwrap_used`,
  `clippy::panic` are denied for non-test builds).
* Every index is bounds-checked; parsing is iterative; all sizes are capped.
* Hostile-input tests feed unterminated tags, enormous entities, deep nesting,
  absurd font sizes, truncated HTTP framing and 30 000 paragraphs.

## Testing strategy

| Layer | How it is tested |
| --- | --- |
| `http` | responses split at every possible boundary, chunked byte-by-byte, malformed status lines, oversized headers/bodies |
| `html` | repair of unclosed tags, entity table, whitespace-only nodes, node/depth budgets, UTF-8 |
| `style`/`stylesheet` | specificity, malformed declarations, rule budget |
| `layout` | wrap measurements, alignment, markers, `pre`, tables, culling, budget, tiny viewports |
| `engine` | redirect loops, unsupported encodings/MIME types, truncation, request-once semantics |
| kernel side | the real `kernel/src/desktop/browser.rs` is type-checked on the host against mock compositor/network modules |

## Change log

* `0.3.0` — initial engine: URL/HTTP/HTML/CSS/layout, integrated browser app,
  idle-skip rendering.
