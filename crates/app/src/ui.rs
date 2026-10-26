//! The expression panel is one HTML page with every script, style and font
//! inlined, so the .exe needs no files beside it.

const INDEX: &str = include_str!("../ui/index.html");
const STYLE: &str = include_str!("../ui/app.css");
const SCRIPT: &str = include_str!("../ui/app.js");
const JQUERY: &str = include_str!("../ui/vendor/jquery.min.js");
const MATHQUILL_JS: &str = include_str!("../ui/vendor/mathquill/mathquill.min.js");
const MATHQUILL_CSS: &str = include_str!("../ui/vendor/mathquill/mathquill.css");
const SYMBOLA_WOFF2: &[u8] = include_bytes!("../ui/vendor/mathquill/Symbola.woff2");

pub fn panel_html() -> String {
    let font = format!("data:font/woff2;base64,{}", base64(SYMBOLA_WOFF2));
    INDEX
        .replace(
            "{{MATHQUILL_CSS}}",
            &MATHQUILL_CSS.replace("__SYMBOLA_WOFF2__", &font),
        )
        .replace("{{STYLE}}", STYLE)
        .replace("{{JQUERY}}", JQUERY)
        .replace("{{MATHQUILL_JS}}", MATHQUILL_JS)
        .replace("{{SCRIPT}}", SCRIPT)
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_reference() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn panel_is_self_contained_and_small_enough() {
        let html = panel_html();
        for name in [
            "{{MATHQUILL_CSS}}",
            "{{STYLE}}",
            "{{JQUERY}}",
            "{{MATHQUILL_JS}}",
            "{{SCRIPT}}",
        ] {
            assert!(!html.contains(name), "unfilled placeholder {name}");
        }
        assert!(!html.contains("__SYMBOLA_WOFF2__"));
        // WebView2 refuses inline pages over 2 MB.
        assert!(html.len() < 2_000_000, "panel is {} bytes", html.len());
    }
}
