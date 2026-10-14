//! Diagnostic: measures how a model's streamed reply actually arrives.
//! Uses the API key saved by the app; the key is never printed.
//!
//!   cargo run --example stream_probe -- [with-tools|no-tools] [model]
//!   cargo run --example stream_probe -- list      (models the gateway offers)

use futures_util::StreamExt;
use serde_json::{json, Value};
use std::time::Instant;

#[tokio::main]
async fn main() {
    let creds = desktop_lib::security::load_credentials_securely();
    if creds.api_key().is_empty() {
        eprintln!("No API key saved in Vivy settings.");
        return;
    }
    let with_tools = std::env::args().nth(1).as_deref() != Some("no-tools");
    if std::env::args().nth(1).as_deref() == Some("list") {
        let url = format!("{}/models", creds.api_base());
        let v: Value = reqwest::Client::new()
            .get(&url)
            .bearer_auth(creds.api_key())
            .send()
            .await
            .expect("request failed")
            .json()
            .await
            .unwrap_or_default();
        for m in v["data"].as_array().cloned().unwrap_or_default() {
            println!("{}", m["id"].as_str().unwrap_or("?"));
        }
        return;
    }
    let model = std::env::args().nth(2).unwrap_or_else(|| creds.model_name.clone());

    let mut body = json!({
        "model": model,
        "stream": true,
        "include_reasoning": true,
        "stream_options": { "include_usage": true },
        "temperature": 0.2,
        "messages": [
            { "role": "system", "content": "You are a helpful assistant." },
            { "role": "user", "content": "Think it through carefully first, then write about 250 words on how to plan a weekend trip on a small budget." }
        ]
    });
    if with_tools {
        body["tools"] = Value::Array(desktop_lib::agent::tools::get_agent_tools_schema());
        body["tool_choice"] = json!("auto");
    }

    println!("model: {model}  tools: {with_tools}");
    let url = format!("{}/chat/completions", creds.api_base());
    let started = Instant::now();
    let resp = reqwest::Client::new()
        .post(&url)
        .bearer_auth(creds.api_key())
        .header("Accept", "text/event-stream")
        .json(&body)
        .send()
        .await
        .expect("request failed");

    println!(
        "status {}  content-type {:?}  headers at {}ms",
        resp.status(),
        resp.headers().get("content-type"),
        started.elapsed().as_millis()
    );
    for h in ["transfer-encoding", "content-encoding", "server", "cf-cache-status", "x-accel-buffering"] {
        if let Some(v) = resp.headers().get(h) {
            println!("  {h}: {v:?}");
        }
    }

    let mut stream = resp.bytes_stream();
    let mut raw = Vec::new();
    let (mut reasoning, mut content, mut events) = (0usize, 0usize, 0usize);
    let mut first_reasoning = None;
    let mut first_content = None;
    let mut chunks = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("stream error");
        chunks += 1;
        let t = started.elapsed().as_millis();
        raw.extend_from_slice(&chunk);
        let mut ev_here = 0;
        while let Some(pos) = raw.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = raw.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line);
            let Some(data) = line.trim().strip_prefix("data:") else { continue };
            let Ok(v) = serde_json::from_str::<Value>(data.trim()) else { continue };
            ev_here += 1;
            let d = &v["choices"][0]["delta"];
            if let Some(r) = d["reasoning"].as_str().or(d["reasoning_content"].as_str()) {
                if !r.is_empty() {
                    reasoning += r.len();
                    first_reasoning.get_or_insert(t);
                }
            }
            if let Some(c) = d["content"].as_str() {
                if !c.is_empty() {
                    content += c.len();
                    first_content.get_or_insert(t);
                }
            }
        }
        events += ev_here;
        println!("+{t:>6}ms {:>6}B  {ev_here:>3} events  (reasoning {reasoning} chars, content {content} chars)", chunk.len());
    }
    println!(
        "done at {}ms: {chunks} chunks, {events} events, first reasoning at {first_reasoning:?}ms, first content at {first_content:?}ms",
        started.elapsed().as_millis()
    );
}
