//! Turns the engine's events into terminal output while a task runs.
//!
//! One `TaskView` owns stdout for the length of a task: streamed thinking and
//! replies, Matsumoto's actions, and a status line at the bottom that shows the
//! mini Matsumoto at work. The status line is only drawn when the cursor is at
//! the start of a line, so it never cuts into streamed text.

use std::collections::HashMap;
use std::io::Write;
use std::time::Instant;

use serde_json::Value;

use crate::config::Look;
use crate::matsumoto;
use crate::term::{self, Rgb};

const VIVY: Rgb = Rgb(120, 170, 255);
const REASONING_PREFIX: &str = "  │ ";

#[derive(PartialEq, Clone, Copy)]
enum Mode {
    None,
    Reasoning,
    Content,
}

#[derive(PartialEq, Clone, Copy)]
enum Speaker {
    Nobody,
    Vivy,
    Matsumoto,
}

enum Status {
    Thinking,
    Tool(String),
}

pub struct TaskView {
    look: Look,
    /// No status line when output goes to a file or pipe.
    live_status: bool,
    out: std::io::Stdout,
    at_line_start: bool,
    status: Option<(Status, Instant)>,
    status_drawn: bool,
    frame: u64,
    mode: Mode,
    speaker: Speaker,
    streamed_content: bool,
    streamed_reasoning: bool,
    md: Markdown,
    wrap: Wrap,
    tools: HashMap<String, (String, Value)>,
    /// How the run ended, for the caller's exit code.
    pub outcome: Option<Outcome>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Outcome {
    Done,
    Failed,
    Stopped,
}

impl TaskView {
    pub fn new(look: Look) -> Self {
        use std::io::IsTerminal;
        Self {
            look,
            live_status: std::io::stdout().is_terminal(),
            out: std::io::stdout(),
            at_line_start: true,
            status: Some((Status::Thinking, Instant::now())),
            status_drawn: false,
            frame: 0,
            mode: Mode::None,
            speaker: Speaker::Nobody,
            streamed_content: false,
            streamed_reasoning: false,
            md: Markdown::default(),
            wrap: Wrap::new(""),
            tools: HashMap::new(),
            outcome: None,
        }
    }

    // ---- Output primitives ------------------------------------------------------

    fn clear_status(&mut self) {
        if self.status_drawn {
            let _ = write!(self.out, "\r\x1b[2K");
            self.status_drawn = false;
        }
    }

    fn print(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.clear_status();
        let _ = write!(self.out, "{text}");
        self.at_line_start = text.ends_with('\n');
        let _ = self.out.flush();
    }

    fn line(&mut self, text: &str) {
        self.end_line();
        self.print(&format!("{text}\n"));
    }

    fn end_line(&mut self) {
        if !self.at_line_start {
            self.print("\n");
        }
    }

    fn speaker(&mut self, who: Speaker) {
        if self.speaker == who {
            return;
        }
        self.end_line();
        let label = match who {
            Speaker::Vivy => term::bold(&term::tint("Vivy", VIVY)),
            Speaker::Matsumoto => {
                let cube = matsumoto::mini(&self.look.palette, 0, term::mode(), self.look.pixel);
                format!("{cube} {}", term::bold(&term::tint("Matsumoto", self.look.palette.body.mix(VIVY, 0.25))))
            }
            Speaker::Nobody => return,
        };
        self.print(&format!("\n{label}\n"));
        self.speaker = who;
    }

    /// Redraw the status line. Called about 8 times a second.
    pub fn tick(&mut self) {
        self.frame += 1;
        if !self.live_status {
            return;
        }
        let Some((status, since)) = &self.status else { return };
        if !self.at_line_start {
            return;
        }
        let secs = since.elapsed().as_secs_f32();
        let text = match status {
            Status::Thinking => "Vivy is thinking".to_string(),
            Status::Tool(desc) => format!("Matsumoto is {desc}"),
        };
        let time = if secs >= 1.0 { format!(" · {secs:.0}s") } else { String::new() };
        // With animation off he holds still; the clock still counts.
        let frame = if self.look.animate { self.frame } else { 0 };
        let cube = matsumoto::mini(&self.look.palette, frame, term::mode(), self.look.pixel);
        let max = term::width().saturating_sub(12);
        let text: String = text.chars().take(max).collect();
        let _ = write!(self.out, "\r\x1b[2K{cube} {}{}", term::dim(&text), term::dim(&time));
        let _ = self.out.flush();
        self.status_drawn = true;
    }

    /// Clear the status line for good (before prompting, or when finished).
    pub fn finish(&mut self) {
        self.status = None;
        self.clear_status();
        self.end_line();
        let _ = self.out.flush();
    }

    /// Hide the status line while the user answers a question.
    pub fn pause_status(&mut self) {
        self.clear_status();
        self.end_line();
    }

    // ---- Events -------------------------------------------------------------------

    pub fn handle(&mut self, event: &str, payload: &Value) {
        match event {
            "agent-stream" => self.on_stream(payload),
            "agent-action" => self.on_action(payload),
            "agent-complete" => self.outcome = Some(Outcome::Done),
            "agent-error" => self.outcome = Some(Outcome::Failed),
            "agent-stopped" => self.outcome = Some(Outcome::Stopped),
            _ => {
                if let Some(note) = phone_notice(event, payload) {
                    self.line(&note);
                }
            }
        }
    }

    fn on_stream(&mut self, p: &Value) {
        let text = p["text"].as_str().unwrap_or("");
        match p["kind"].as_str().unwrap_or("") {
            "start" => {
                // No status line while text streams; it would reset the styling.
                self.status = None;
                self.streamed_content = false;
                self.streamed_reasoning = false;
                self.mode = Mode::None;
                self.md = Markdown::default();
                self.wrap = Wrap::new("");
            }
            "reasoning" => {
                if self.mode != Mode::Reasoning {
                    self.speaker(Speaker::Vivy);
                    self.end_line();
                    self.print(&term::code("2;3"));
                    self.print(REASONING_PREFIX);
                    self.mode = Mode::Reasoning;
                    self.wrap = Wrap::new(REASONING_PREFIX);
                }
                self.streamed_reasoning = true;
                let body = self.wrap.push(text);
                self.print(&body);
            }
            "content" => {
                if self.mode == Mode::Reasoning {
                    let rest = self.wrap.flush();
                    self.print(&rest);
                    self.print(&term::code("0"));
                    self.end_line();
                }
                if self.mode != Mode::Content {
                    self.speaker(Speaker::Vivy);
                    self.mode = Mode::Content;
                    self.wrap = Wrap::new("");
                }
                self.streamed_content = true;
                let styled = self.md.push(text);
                let wrapped = self.wrap.push(&styled);
                self.print(&wrapped);
            }
            "end" => {
                let rest = self.md.flush();
                let rest = format!("{}{}", self.wrap.push(&rest), self.wrap.flush());
                self.print(&rest);
                if self.mode == Mode::Reasoning {
                    self.print(&term::code("0"));
                }
                self.mode = Mode::None;
                self.end_line();
                self.status = Some((Status::Thinking, Instant::now()));
            }
            _ => {}
        }
    }

    fn on_action(&mut self, log: &Value) {
        let text = log["description"].as_str().unwrap_or("");
        let d = &log["details"];
        match log["action_type"].as_str().unwrap_or("") {
            "reasoning" if !self.streamed_reasoning => {
                self.speaker(Speaker::Vivy);
                let shown: Vec<&str> = text.lines().take(8).collect();
                let more = if text.lines().count() > 8 { format!("\n{REASONING_PREFIX}…") } else { String::new() };
                self.line(&term::italic_dim(&format!("{REASONING_PREFIX}{}{more}", shown.join(&format!("\n{REASONING_PREFIX}")))));
            }
            "thought" if !self.streamed_content => self.say(text),
            "tool_call" => {
                let tool = d["tool"].as_str().unwrap_or(text).to_string();
                if tool == "finish_task" {
                    return;
                }
                let args = d["arguments"].clone();
                self.status = Some((Status::Tool(describe(&tool, &args)), Instant::now()));
                if let Some(id) = d["call_id"].as_str() {
                    self.tools.insert(id.to_string(), (tool, args));
                }
            }
            "tool_result" => {
                let Some((tool, args)) = d["call_id"].as_str().and_then(|id| self.tools.remove(id)) else { return };
                self.speaker(Speaker::Matsumoto);
                let secs = d["ms"].as_u64().map(|ms| format!("  {:.1}s", ms as f32 / 1000.0)).unwrap_or_default();
                if d["ok"].as_bool().unwrap_or(false) {
                    let detail = summarize(&tool, &args);
                    self.line(&format!("  {} {}{}{}", term::green("✓"), past_tense(&tool, &args), term::dim(&detail), term::dim(&secs)));
                } else {
                    let why = text.trim_start_matches("ERROR:").trim().lines().next().unwrap_or("").to_string();
                    let why: String = why.chars().take(140).collect();
                    self.line(&format!("  {} {} {}{}", term::red("✗"), describe(&tool, &args), term::dim(&format!("failed: {why}")), term::dim(&secs)));
                }
                self.status = Some((Status::Thinking, Instant::now()));
            }
            "finished" => {
                let is_reply = d.get("reply").is_some();
                if is_reply {
                    if !self.streamed_content {
                        self.say(text);
                    }
                } else {
                    // finish_task's summary: Vivy reporting back.
                    self.speaker(Speaker::Vivy);
                    let styled = format!("✓ {}{}", self.md.push(text), self.md.flush());
                    let styled = wrap_all(&styled, "");
                    self.line(&styled.trim_end().replacen('✓', &term::green("✓"), 1));
                }
                self.status = None;
            }
            "error" => {
                self.end_line();
                self.line(&format!("{} {}", term::red("✗"), term::red(text)));
                self.status = None;
            }
            "stopped" => {
                self.end_line();
                self.line(&term::yellow("■ Stopped."));
                self.status = None;
            }
            "compacted" => self.line(&term::dim(&format!("  ({text})"))),
            "permission_request" => {} // the prompt itself comes as its own event
            _ => {}
        }
    }

    /// Vivy saying something that didn't stream.
    fn say(&mut self, text: &str) {
        self.speaker(Speaker::Vivy);
        let mut md = Markdown::default();
        let styled = format!("{}{}", md.push(text), md.flush());
        self.line(wrap_all(&styled, "").trim_end());
    }
}

/// One-line notices about the phone, shown during tasks and at the prompt.
pub fn phone_notice(event: &str, p: &Value) -> Option<String> {
    match event {
        "phone-connected" => {
            let name = p["device_name"].as_str().filter(|s| !s.is_empty()).unwrap_or("phone");
            Some(term::green(&format!("● {name} connected")))
        }
        "phone-disconnected" => Some(term::yellow("○ Phone disconnected")),
        "phone-error" => p["error"].as_str().map(|e| term::yellow(&format!("! {e}"))),
        "phone-firewall" => p["message"]
            .as_str()
            .map(|m| term::yellow(&format!("! {m} Fix it with: vivy firewall"))),
        "pairing-changed" => {
            let name = p["paired"]["device_name"].as_str()?;
            Some(term::green(&format!("● Paired with {name}")))
        }
        "phone-status" => match (p["accessibility"].as_bool(), p["screen_share"].as_bool()) {
            (Some(false), _) => Some(term::yellow("! Accessibility is off on the phone, so Matsumoto can't tap or type.")),
            _ => None,
        },
        _ => None,
    }
}

// ---- Describing tools -----------------------------------------------------------

fn quoted(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.trim().is_empty()).map(|s| format!("“{}”", s.trim()))
}

/// Present tense, for the status line: "tapping “Search”".
pub fn describe(tool: &str, a: &Value) -> String {
    match tool {
        "inspect_screen" => "reading the screen".into(),
        "tap" | "tap_element" => format!("tapping {}", quoted(&a["text"]).or_else(|| quoted(&a["element_label"])).unwrap_or("the screen".into())),
        "long_press" => "pressing and holding".into(),
        "swipe" => "swiping".into(),
        "scroll" => format!("scrolling {}", a["direction"].as_str().unwrap_or("down").to_lowercase()),
        "type_text" => "typing".into(),
        "press_key" => format!("pressing {}", a["key"].as_str().unwrap_or("a key").to_lowercase()),
        "open_app" => format!("opening {}", a["app_name"].as_str().or(a["package_name"].as_str()).unwrap_or("an app")),
        "wait" => "waiting for the phone".into(),
        "web_search" => "searching the web".into(),
        "ask_permission" => "waiting for your OK".into(),
        other => format!("running {other}"),
    }
}

/// Past tense, for the finished line: "tapped".
fn past_tense(tool: &str, a: &Value) -> String {
    match tool {
        "tap" | "tap_element" => "tapped".into(),
        "long_press" => "held".into(),
        "swipe" => "swiped".into(),
        "scroll" => "scrolled".into(),
        "type_text" => "typed".into(),
        "press_key" => format!("pressed {}", a["key"].as_str().unwrap_or("a key").to_lowercase()),
        "open_app" => format!("opened {}", a["app_name"].as_str().or(a["package_name"].as_str()).unwrap_or("an app")),
        "wait" => "waited".into(),
        "inspect_screen" => "read the screen".into(),
        "web_search" => "searched the web".into(),
        "ask_permission" => "asked you".into(),
        other => other.into(),
    }
}

/// The short detail after the past tense: what was tapped, typed, searched.
fn summarize(tool: &str, a: &Value) -> String {
    let detail = match tool {
        "tap_element" => quoted(&a["text"]),
        "tap" | "long_press" => match (a["x"].as_f64(), a["y"].as_f64()) {
            (Some(x), Some(y)) => Some(format!("at {x:.0}%, {y:.0}%")),
            _ => quoted(&a["element_label"]),
        },
        "type_text" => quoted(&a["text"]).map(|t| t.chars().take(60).collect()),
        "scroll" => a["direction"].as_str().map(|d| d.to_lowercase()),
        "web_search" => quoted(&a["query"]),
        "inspect_screen" => quoted(&a["target"]).map(|t| format!("for {t}")),
        _ => None,
    };
    detail.map(|d| format!(" {d}")).unwrap_or_default()
}

// ---- Word wrapping --------------------------------------------------------------

/// Columns a character takes up: 2 for wide CJK and emoji, 0 for joiners.
fn char_width(c: char) -> usize {
    match c as u32 {
        0x200B..=0x200D | 0xFE00..=0xFE0F => 0,
        0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x1F300..=0x1FAFF | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

/// Wraps streamed text at word boundaries so words aren't split at the edge
/// of the terminal. Escape codes pass through and take no space. Text after
/// each line break is indented with `indent`.
pub struct Wrap {
    indent: &'static str,
    width: usize,
    col: usize,
    word: String,
    word_w: usize,
}

impl Wrap {
    pub fn new(indent: &'static str) -> Self {
        Self::with_width(indent, term::width().saturating_sub(1))
    }

    fn with_width(indent: &'static str, width: usize) -> Self {
        let col = term::visible_width(indent);
        Self { indent, width: width.max(col + 10), col, word: String::new(), word_w: 0 }
    }

    fn break_line(&mut self, out: &mut String) {
        out.push('\n');
        out.push_str(self.indent);
        self.col = term::visible_width(self.indent);
    }

    fn flush_word(&mut self, out: &mut String) {
        if self.word.is_empty() {
            return;
        }
        let start = term::visible_width(self.indent);
        if self.col + self.word_w > self.width && self.col > start {
            self.break_line(out);
        }
        out.push_str(&self.word);
        // A word longer than a whole line wraps on its own in the terminal.
        self.col = (self.col + self.word_w).min(self.width);
        self.word.clear();
        self.word_w = 0;
    }

    pub fn push(&mut self, text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\x1b' => {
                    // Copy the whole escape sequence; it has no width.
                    self.word.push(c);
                    for e in chars.by_ref() {
                        self.word.push(e);
                        if e.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                '\n' => {
                    self.flush_word(&mut out);
                    self.break_line(&mut out);
                }
                ' ' => {
                    self.flush_word(&mut out);
                    if self.col + 1 >= self.width {
                        self.break_line(&mut out);
                    } else if self.col > term::visible_width(self.indent) {
                        out.push(' ');
                        self.col += 1;
                    }
                }
                _ => {
                    self.word.push(c);
                    self.word_w += char_width(c);
                }
            }
        }
        out
    }

    /// Release the last word (end of the text).
    pub fn flush(&mut self) -> String {
        let mut out = String::new();
        self.flush_word(&mut out);
        out
    }
}

/// Wrap a whole piece of text at once.
fn wrap_all(text: &str, indent: &'static str) -> String {
    let mut w = Wrap::new(indent);
    let mut out = w.push(text);
    out.push_str(&w.flush());
    out
}

// ---- Markdown, streamed -------------------------------------------------------

/// Styles `**bold**`, `` `code` `` and `# headings` in text that arrives in
/// pieces. A `*` or `` ` `` at the end of a piece is held back until the next
/// one shows whether it starts a marker.
#[derive(Default)]
pub struct Markdown {
    bold: bool,
    code: bool,
    heading: bool,
    held: String,
    line_start: bool,
    started: bool,
}

impl Markdown {
    pub fn push(&mut self, text: &str) -> String {
        if !self.started {
            self.started = true;
            self.line_start = true;
        }
        let mut input = std::mem::take(&mut self.held);
        input.push_str(text);
        let chars: Vec<char> = input.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            let rest = chars.len() - i;
            if self.line_start && !self.code {
                // Headings: "# ", "## ", "### "
                let hashes = chars[i..].iter().take_while(|&&c| c == '#').count();
                if hashes > 0 && hashes <= 4 {
                    if i + hashes >= chars.len() {
                        self.held = chars[i..].iter().collect();
                        return out;
                    }
                    if chars[i + hashes] == ' ' {
                        out.push_str(&term::code("1"));
                        self.heading = true;
                        i += hashes + 1;
                        self.line_start = false;
                        continue;
                    }
                }
            }
            self.line_start = false;
            match c {
                '*' if !self.code => {
                    if rest == 1 {
                        self.held.push('*');
                        return out;
                    }
                    if chars[i + 1] == '*' {
                        self.bold = !self.bold;
                        out.push_str(&term::code(if self.bold { "1" } else { "22" }));
                        i += 2;
                        continue;
                    }
                    out.push('*');
                }
                '`' => {
                    self.code = !self.code;
                    out.push_str(&term::code(if self.code { "36" } else { "39" }));
                }
                '\n' => {
                    if self.heading {
                        out.push_str(&term::code("22"));
                        self.heading = false;
                    }
                    out.push('\n');
                    self.line_start = true;
                }
                _ => out.push(c),
            }
            i += 1;
        }
        out
    }

    /// End of the text: release anything held and reset styles.
    pub fn flush(&mut self) -> String {
        let mut out = std::mem::take(&mut self.held);
        if self.bold || self.code || self.heading {
            out.push_str(&term::code("0"));
        }
        *self = Markdown::default();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(chunks: &[&str]) -> String {
        crate::term::init("none");
        let mut md = Markdown::default();
        let mut out: String = chunks.iter().map(|c| md.push(c)).collect();
        out.push_str(&md.flush());
        out
    }

    #[test]
    fn markers_split_across_chunks_still_vanish() {
        assert_eq!(plain(&["Hello **wor", "ld*", "* and `co", "de`"]), "Hello world and code");
        assert_eq!(plain(&["# Ti", "tle\nbody"]), "Title\nbody");
        assert_eq!(plain(&["a * b"]), "a * b");
        assert_eq!(plain(&["ends with *"]), "ends with *");
    }

    #[test]
    fn wraps_at_words_across_chunks() {
        crate::term::init("none");
        let mut w = Wrap::with_width("", 20);
        let mut out: String = ["Hi there! I can open apps, se", "nd messages and search the web."]
            .iter()
            .map(|c| w.push(c))
            .collect();
        out.push_str(&w.flush());
        for line in out.lines() {
            assert!(line.chars().count() <= 20, "{line:?}");
        }
        assert_eq!(out.split_whitespace().collect::<Vec<_>>().join(" "), "Hi there! I can open apps, send messages and search the web.");
        assert!(out.contains("send"), "a word split across chunks stays whole");

        let mut w = Wrap::with_width("  | ", 20);
        let out = format!("{}{}", w.push("one two three four five six"), w.flush());
        assert!(out.lines().skip(1).all(|l| l.starts_with("  | ")), "{out:?}");

        let mut w = Wrap::with_width("", 20);
        let out = format!("{}{}", w.push("\x1b[1mbold\x1b[22m word"), w.flush());
        assert!(!out.contains('\n'), "escape codes take no space");
    }

    #[test]
    fn describes_tools_like_the_desktop_app() {
        let a = serde_json::json!({ "text": "Search" });
        assert_eq!(describe("tap_element", &a), "tapping “Search”");
        assert_eq!(past_tense("open_app", &serde_json::json!({ "app_name": "YouTube" })), "opened YouTube");
        assert_eq!(summarize("tap_element", &a), " “Search”");
    }
}
