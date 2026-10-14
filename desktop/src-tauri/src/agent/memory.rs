//! Keeps a chat's remembered conversation small: once it grows past a budget,
//! the older turns are folded into a model-written summary.

use std::time::Duration;
use serde_json::{json, Value};

use crate::security::ApiCredentials;

/// Roughly 15k tokens of remembered conversation before compacting.
const COMPACT_AT_CHARS: usize = 60_000;
/// Exchanges (user + assistant pairs) always kept word for word.
const KEEP_RECENT_TURNS: usize = 6;
/// If summarizing keeps failing, drop the oldest turns past this instead.
const HARD_LIMIT_CHARS: usize = 200_000;

fn turn_chars(turns: &[Value]) -> usize {
    turns.iter().map(|t| t["content"].as_str().map_or(0, str::len)).sum()
}

pub enum Compaction {
    /// Nothing to do.
    None,
    /// Fold the first `folded` turns into this summary.
    Summarized { summary: String, folded: usize },
    /// Summarizing failed and the chat was huge; drop the first `folded` turns.
    Dropped { folded: usize },
}

/// Decide whether this conversation needs compacting and, if so, produce the summary.
pub async fn compact(creds: &ApiCredentials, summary: Option<&str>, turns: &[Value]) -> Compaction {
    let size = summary.map_or(0, str::len) + turn_chars(turns);
    let keep = KEEP_RECENT_TURNS * 2;
    if size <= COMPACT_AT_CHARS || turns.len() <= keep {
        return Compaction::None;
    }

    // Fold everything but the recent exchanges; always cut on a user/assistant boundary.
    let mut folded = turns.len() - keep;
    folded -= folded % 2;
    if folded == 0 {
        return Compaction::None;
    }

    let mut transcript = String::new();
    for t in &turns[..folded] {
        let who = if t["role"] == "user" { "User" } else { "Vivy" };
        transcript.push_str(&format!("{who}: {}\n\n", t["content"].as_str().unwrap_or("")));
    }

    match summarize(creds, summary, &transcript).await {
        Ok(s) if !s.trim().is_empty() => Compaction::Summarized { summary: s.trim().to_string(), folded },
        _ if size > HARD_LIMIT_CHARS => Compaction::Dropped { folded },
        _ => Compaction::None,
    }
}

async fn summarize(creds: &ApiCredentials, previous: Option<&str>, transcript: &str) -> Result<String, String> {
    let instructions = "You keep the long-term memory of a chat between a user and Vivy, an AI assistant that \
        operates the user's Android phone. Write a concise summary (at most about 250 words) of everything worth \
        remembering from the conversation below, merged with the existing summary if there is one: the user's \
        goals and preferences, facts learned, what was done on the phone and how it turned out, and anything \
        still unfinished. Use short plain sentences or bullet points. Write only the summary.";
    let mut content = String::new();
    if let Some(p) = previous {
        content.push_str(&format!("Existing summary:\n{p}\n\n"));
    }
    content.push_str(&format!("Conversation to fold in:\n{transcript}"));

    let mut body = json!({
        "model": creds.model_name,
        "messages": [
            { "role": "system", "content": instructions },
            { "role": "user", "content": content }
        ],
        "temperature": 0.2,
        "max_tokens": 700
    });
    creds.fit_request(&mut body);

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(format!("{}/chat/completions", creds.api_base()))
        .bearer_auth(creds.api_key())
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("summary request failed: {}", resp.status()));
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    let text = v["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string();
    // Some models put their thinking inline; keep only what follows it.
    Ok(match text.rfind("</think>") {
        Some(i) => text[i + "</think>".len()..].to_string(),
        None => text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn small_conversations_are_left_alone() {
        let creds = ApiCredentials::default();
        let turns = vec![json!({"role":"user","content":"hi"}), json!({"role":"assistant","content":"hello"})];
        assert!(matches!(compact(&creds, None, &turns).await, Compaction::None));
    }
}
