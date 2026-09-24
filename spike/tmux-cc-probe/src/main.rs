//! Throwaway Phase 0 probe — spike/tmux-cc-gateway-detection.
//!
//! Question under test: does the real nyaterm output-decode chain preserve the
//! tmux control-mode entry sequence `\x1bP1000p`, or does it swallow it?
//!
//! The real pipeline in `src-tauri/src/core/ssh/io.rs` is:
//!
//!     raw bytes -> TerminalOutputDecoder::decode -> OscStripper::push
//!                -> SessionOutputCoalescer::push_owned -> Tauri event
//!
//! This probe drives the first two stages against the REAL `osc.rs` (included
//! verbatim via #[path]) and a byte-for-byte copy of `TerminalOutputDecoder`
//! from `src-tauri/src/core/terminal_session/mod.rs`.
//!
//! It deliberately does NOT cover the coalescer, the Tauri event layer, or
//! xterm.js. Those are Phase 0 steps L2/L3.

#[path = "../../../src-tauri/src/core/ssh/osc.rs"]
mod osc;

use encoding_rs::{CoderResult, Decoder, Encoding, UTF_8};
use osc::OscStripper;

const READY_MARKER: &str = "\x1b]1337;NyaTermReady=session-probe\x07";

/// Byte-for-byte copy of `TerminalOutputDecoder` (terminal_session/mod.rs).
struct TerminalOutputDecoder {
    decoder: Decoder,
}

impl TerminalOutputDecoder {
    fn new(encoding: &str) -> Self {
        let trimmed = encoding.trim();
        let enc = if trimmed.is_empty() {
            UTF_8
        } else {
            Encoding::for_label(trimmed.as_bytes()).unwrap_or(UTF_8)
        };
        Self {
            decoder: enc.new_decoder_without_bom_handling(),
        }
    }

    fn decode(&mut self, data: &[u8]) -> String {
        let capacity = self
            .decoder
            .max_utf8_buffer_length(data.len())
            .unwrap_or_else(|| data.len().saturating_mul(4));
        let mut output = String::with_capacity(capacity);
        let mut total_read = 0;
        while total_read < data.len() {
            let (result, read, _) =
                self.decoder
                    .decode_to_string(&data[total_read..], &mut output, false);
            total_read += read;
            match result {
                CoderResult::InputEmpty => break,
                CoderResult::OutputFull => {
                    output.reserve(
                        self.decoder
                            .max_utf8_buffer_length(data.len() - total_read)
                            .unwrap_or_else(|| (data.len() - total_read).saturating_mul(4))
                            .max(4),
                    );
                }
            }
        }
        output
    }
}

fn escape(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '\x1b' => out.push_str("\\e"),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            '\x07' => out.push_str("\\a"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    if out.is_empty() {
        "<empty>".to_string()
    } else {
        out
    }
}

struct Outcome {
    visible: String,
    after_ready: String,
    leftover: String,
}

fn drive(chunks: &[&[u8]]) -> Outcome {
    let mut decoder = TerminalOutputDecoder::new("utf-8");
    let mut stripper = OscStripper::new(READY_MARKER);
    let mut visible = String::new();
    let mut after_ready = String::new();
    for chunk in chunks {
        let text = decoder.decode(chunk);
        let result = stripper.push(&text);
        visible.push_str(&result.visible);
        after_ready.push_str(&result.visible_after_ready);
    }
    let leftover = stripper.flush();
    Outcome {
        visible,
        after_ready,
        leftover,
    }
}

fn report(name: &str, chunks: &[&[u8]]) {
    let o = drive(chunks);
    let saw_marker = o.visible.contains("P1000p") || o.visible.contains("\x1bP");
    let protocol_visible = o.visible.contains("%output") || o.visible.contains("%begin");
    println!("── {name}");
    println!("   通过(visible): {}", escape(&o.visible));
    if !o.after_ready.is_empty() {
        println!("   就绪后可见  : {}", escape(&o.after_ready));
    }
    println!("   残留缓冲    : {}", escape(&o.leftover));
    println!(
        "   DCS 存活? {}   |  协议行可见? {}",
        if saw_marker { "是" } else { "否" },
        if protocol_visible { "是" } else { "否" }
    );
    println!();
}

fn main() {
    // Realistic `tmux -CC` opening: 7-byte DCS marker (NO ST terminator),
    // then control-protocol lines.
    const BOOTSTRAP: &[u8] = b"\x1bP1000p%begin 1700000000 1 1\r\n%end 1700000000 1 1\r\n%output %0 hello\r\n";
    const DCS_ONLY: &[u8] = b"\x1bP1000p";
    const DCS_WITH_ST: &[u8] = b"\x1bP1000p\x1b\\%output %0 hello\r\n";
    // Control group: a real OSC that the stripper is designed to remove.
    const OSC7: &[u8] = b"\x1b]7;file://host/tmp/dir\x07";

    println!("=== tmux -CC 网关 Phase 0 探针（真实 osc.rs）===\n");

    report("A. 整块 bootstrap（DCS 无 ST + 协议行）", &[BOOTSTRAP]);

    report("B. 逻辑行分片", &[
        b"\x1bP1000p",
        b"%begin 1700000000 1 1\r\n",
        b"%end 1700000000 1 1\r\n",
        b"%output %0 hello\r\n",
    ]);

    report("C. 逐字节分片（最坏边界）", &{
        let parts: Vec<&[u8]> = BOOTSTRAP.chunks(1).collect();
        // leak into a Vec<&[u8]> with 'static lifetime via BOOTSTRAP
        parts
    });

    report("D. 仅 7 字节 DCS 标记", &[DCS_ONLY]);

    report("E. DCS + 显式 ST 终止符", &[DCS_WITH_ST]);

    report("F. 对照组：OSC 7（应被剥离）", &[OSC7]);

    report("G. 先到就绪标记，再到 DCS + 协议行", &[
        READY_MARKER.as_bytes(),
        BOOTSTRAP,
    ]);
}
