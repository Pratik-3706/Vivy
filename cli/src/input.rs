//! Line input on its own thread (rustyline blocks), with history and a way
//! to print notices above the prompt while the user is typing.

use std::sync::mpsc as std_mpsc;

use std::borrow::Cow;

use rustyline::completion::Completer;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::{Config, Editor, ExternalPrinter, Helper};
use tokio::sync::{mpsc, oneshot};

pub enum Answer {
    Line(String),
    /// Ctrl+C
    Interrupted,
    /// Ctrl+D, or input closed
    Eof,
}

struct Request {
    prompt: String,
    remember: bool,
    reply: oneshot::Sender<Answer>,
}

pub struct Input {
    requests: std_mpsc::Sender<Request>,
    notices: Option<mpsc::UnboundedSender<String>>,
}

/// Colours the prompt's `›`. Escape codes have to go through the highlighter:
/// put straight into the prompt string, rustyline counts them as visible
/// characters and puts the cursor in the wrong place.
struct PromptStyle {
    arrow: String,
}

impl Helper for PromptStyle {}
impl Completer for PromptStyle {
    type Candidate = String;
}
impl Hinter for PromptStyle {
    type Hint = String;
}
impl Validator for PromptStyle {}
impl Highlighter for PromptStyle {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(&'s self, prompt: &'p str, _default: bool) -> Cow<'b, str> {
        if prompt.contains('›') {
            Cow::Owned(prompt.replacen('›', &self.arrow, 1))
        } else {
            Cow::Borrowed(prompt)
        }
    }
}

const HISTORY_FILE: &str = "cli_history.txt";
const HISTORY_SIZE: usize = 500;

impl Input {
    /// `arrow` is the coloured `›` shown in prompts.
    pub fn start(arrow: String) -> Self {
        let (requests, rx) = std_mpsc::channel::<Request>();
        let (printer_tx, printer_rx) = std_mpsc::channel::<Option<mpsc::UnboundedSender<String>>>();

        std::thread::spawn(move || {
            let config = Config::builder().max_history_size(HISTORY_SIZE).ok().map(|b| b.auto_add_history(false).build());
            let mut rl: Editor<PromptStyle, DefaultHistory> = match Editor::with_config(config.unwrap_or_default()) {
                Ok(rl) => rl,
                Err(_) => {
                    let _ = printer_tx.send(None);
                    // No line editor: answer every request with plain stdin.
                    for req in rx {
                        let _ = req.reply.send(read_plain(&req.prompt));
                    }
                    return;
                }
            };
            rl.set_helper(Some(PromptStyle { arrow }));
            let history = vivy_core::paths::data_path(HISTORY_FILE);
            if let Some(h) = &history {
                let _ = rl.load_history(h);
            }

            // Notices go through rustyline's printer so they appear above the prompt.
            match rl.create_external_printer() {
                Ok(mut printer) => {
                    let (tx, mut notice_rx) = mpsc::unbounded_channel::<String>();
                    let _ = printer_tx.send(Some(tx));
                    std::thread::spawn(move || {
                        while let Some(msg) = notice_rx.blocking_recv() {
                            let _ = printer.print(msg);
                        }
                    });
                }
                Err(_) => {
                    let _ = printer_tx.send(None);
                }
            }

            for req in rx {
                let answer = match rl.readline(&req.prompt) {
                    Ok(line) => {
                        if req.remember && !line.trim().is_empty() {
                            let _ = rl.add_history_entry(line.as_str());
                            if let Some(h) = &history {
                                let _ = rl.save_history(h);
                            }
                        }
                        Answer::Line(line)
                    }
                    Err(ReadlineError::Interrupted) => Answer::Interrupted,
                    Err(_) => Answer::Eof,
                };
                let _ = req.reply.send(answer);
            }
        });

        let notices = printer_rx.recv().ok().flatten();
        Self { requests, notices }
    }

    /// Show `prompt` and wait for a line. `remember` adds it to history.
    pub async fn ask(&self, prompt: &str, remember: bool) -> Answer {
        let (reply, rx) = oneshot::channel();
        let req = Request { prompt: prompt.to_string(), remember, reply };
        if self.requests.send(req).is_err() {
            return Answer::Eof;
        }
        rx.await.unwrap_or(Answer::Eof)
    }

    /// Print a line above the prompt without disturbing what's being typed.
    pub fn notice(&self, text: String) {
        match &self.notices {
            Some(tx) => {
                let _ = tx.send(format!("{text}\n"));
            }
            None => println!("\r{text}"),
        }
    }
}

fn read_plain(prompt: &str) -> Answer {
    use std::io::Write;
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) | Err(_) => Answer::Eof,
        Ok(_) => Answer::Line(line.trim_end_matches(['\r', '\n']).to_string()),
    }
}
