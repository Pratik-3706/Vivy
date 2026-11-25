use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct TavilyRequest<'a> {
    api_key: &'a str,
    query: &'a str,
    search_depth: &'a str,
    include_answer: bool,
    max_results: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TavilySearchResult {
    pub title: String,
    pub url: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
struct TavilyResponse {
    pub answer: Option<String>,
    pub results: Option<Vec<TavilySearchResult>>,
}

#[derive(Debug, Serialize)]
pub struct SearchSummary {
    pub answer: Option<String>,
    pub results: Vec<TavilySearchResult>,
}

pub async fn execute_tavily_search(
    api_key: &str,
    query: &str,
    max_results: u32,
) -> Result<SearchSummary, String> {
    if api_key.trim().is_empty() {
        return Err("Tavily API key is not configured in Settings".to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let payload = TavilyRequest {
        api_key,
        query,
        search_depth: "basic",
        include_answer: true,
        max_results: max_results.clamp(1, 10),
    };

    let resp = client
        .post("https://api.tavily.com/search")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to reach Tavily API: {e}"))?;

    if !resp.status().is_success() {
        let err_text = resp.text().await.unwrap_or_default();
        return Err(format!("Tavily search failed: {err_text}"));
    }

    let parsed: TavilyResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse Tavily response: {e}"))?;

    Ok(SearchSummary {
        answer: parsed.answer,
        results: parsed.results.unwrap_or_default(),
    })
}
