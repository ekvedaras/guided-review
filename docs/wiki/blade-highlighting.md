# Blade highlighting

2026-11-12. A `*.blade.php` file used to get the PHP grammar. A Blade file has no `<?php`, so all of it was one HTML text node and nothing was coloured.

- **No grammar to use.** crates.io has no `tree-sitter-blade`. The upstream grammars are not published and are not checked against tree-sitter 0.27, so adding one would mean vendoring generated C.
- **Two layers instead** (`src/viewer/highlight/blade.rs`, `highlight_blade`). A scan finds the Blade parts: `{{ }}`, `{!! !!}`, `{{-- --}}`, directives with their arguments, and `@php` blocks. The scan blanks those parts byte for byte, so the HTML grammar colours the rest with the same offsets. Directive names are keywords, delimiters are special punctuation, and the PHP inside is coloured by the PHP grammar from a stand-in statement (`<?php foreach (…);`, `<?php …;`), keeping only the spans inside the original range.
- **Only Laravel's directives** count (a list in `blade.rs`), so `@media` in a `<style>` and Alpine's `@click` stay HTML. `me@example.com` and `@@escaped` stay HTML too.
- **No injection support was added** to `highlight_file`: the scan replaces what an injection query would have said, and compiles nothing new (Blade reuses the HTML and PHP configurations, each compiled once).
- **Symbols and the outline are unchanged**: `Language::for_path` still says PHP for `.blade.php`, which finds no symbols in a template, as before.

Known open: `@verbatim` bodies are still scanned for Blade parts; a directive's arguments that are not valid PHP once wrapped (`@case`) are coloured by error recovery.
