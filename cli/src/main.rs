//! `vivy`: Vivy in the terminal, for Windows and Linux.
//!
//! The same agent as the desktop app (shared through `vivy-core`): chat with
//! Vivy, and Matsumoto carries out phone tasks. Settings, keys, chats and the
//! paired phone are shared with the desktop app on the same machine.

mod app;
mod commands;
mod config;
mod input;
mod matsumoto;
mod render;
mod term;

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "vivy",
    version,
    about = "Tell your Android phone what to do, from the terminal.",
    long_about = "Vivy plans, and her cube helper Matsumoto taps through it on your phone.\n\
                  Run `vivy` for a chat, or `vivy run \"...\"` for a single task.\n\
                  First time? `vivy key` to add an API key, then `vivy pair` to connect your phone.",
    after_help = "Settings and chats are shared with the Vivy desktop app on this computer."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Turn off colours (same as NO_COLOR=1).
    #[arg(long, global = true)]
    no_color: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Chat with Vivy (the default when no command is given).
    Chat(ChatArgs),
    /// Do one task, then exit. Exit code 0 = done, 1 = failed, 130 = stopped.
    Run(RunArgs),
    /// Pair your phone, or show the paired phone.
    Pair {
        /// Show a new code even if a phone is paired (to switch phones).
        #[arg(long)]
        new: bool,
    },
    /// Unpair the phone. It will need a new code to connect again.
    Forget,
    /// Let your phone through Windows Firewall (asks Windows for admin rights).
    Firewall,
    /// Provider, model, phone and where everything is stored.
    Status,
    /// Save an API key. The key is typed hidden and stored in the system keyring.
    Key {
        /// Provider (aicredits, openai, anthropic, google, custom) or "tavily"
        /// for web search. Defaults to the provider in use.
        provider: Option<String>,
    },
    /// Switch provider: aicredits, openai, anthropic, google or custom.
    Use {
        provider: String,
        /// Model to use with it (defaults to its everyday model).
        model: Option<String>,
        /// Endpoint for the custom provider, e.g. https://openrouter.ai/api/v1
        #[arg(long)]
        url: Option<String>,
    },
    /// List the provider's models, or pick one by number or ID.
    Model { model: Option<String> },
    /// Show or set how hard the model thinks (depends on the provider).
    Effort { level: Option<String> },
    /// List saved chats.
    Chats,
    /// Show Matsumoto and change how he looks.
    Matsumoto(commands::MatsumotoArgs),
}

#[derive(Args, Default)]
struct ChatArgs {
    /// Continue the most recent chat instead of starting a new one.
    #[arg(short, long)]
    resume: bool,
}

#[derive(Args)]
struct RunArgs {
    /// What to do, e.g. "turn on Wi-Fi".
    #[arg(required = true, num_args = 1..)]
    task: Vec<String>,
    /// Seconds to wait for a paired phone to connect first.
    #[arg(long, default_value_t = 20)]
    wait: u64,
    /// Continue the most recent chat, so Vivy remembers it.
    #[arg(short, long)]
    resume: bool,
}

fn main() {
    let cli = Cli::parse();
    let cfg = config::CliConfig::load();
    term::init(if cli.no_color { "none" } else { &cfg.terminal.color });

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("couldn't start the async runtime");

    let code = runtime.block_on(async move {
        match cli.command.unwrap_or(Command::Chat(ChatArgs::default())) {
            Command::Chat(a) => app::chat(cfg, a.resume).await,
            Command::Run(a) => app::run_once(cfg, a.task.join(" "), a.wait, a.resume).await,
            Command::Pair { new } => commands::pair(cfg, new).await,
            Command::Forget => commands::forget(),
            Command::Firewall => commands::firewall(),
            Command::Status => commands::status(),
            Command::Key { provider } => commands::key(provider.as_deref()),
            Command::Use { provider, model, url } => commands::use_provider(&provider, model.as_deref(), url.as_deref()),
            Command::Model { model } => commands::model(model.as_deref()),
            Command::Effort { level } => commands::effort(level.as_deref()),
            Command::Chats => commands::chats(),
            Command::Matsumoto(a) => commands::matsumoto(cfg, a).await,
        }
    });
    // Don't wait for the phone server and other background work to wind down.
    runtime.shutdown_background();
    std::process::exit(code);
}
