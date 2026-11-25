use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize, Deserialize)]
pub struct ScreenInspectionResult {
    pub current_app_or_view: String,
    pub description: String,
    pub raw_analysis: String,
}

pub async fn analyze_screen_frame(
    creds: &crate::security::ApiCredentials,
    model_name: &str,
    base64_frame: &str,
    target_query: Option<&str>,
) -> Result<String, String> {
    let api_key = creds.api_key();
    let base_url = creds.api_base();
    if api_key.is_empty() {
        return Err("API key is not configured in Settings".to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let image_data_url = if base64_frame.starts_with("data:") {
        base64_frame.to_string()
    } else {
        format!("data:image/jpeg;base64,{}", base64_frame)
    };

    let user_prompt = match target_query {
        Some(query) => format!(
            "You are a mobile UI grounding engine for Android automation. \
             Carefully examine this live Android phone screen.\n\
             Task: Find the exact element matching: \"{query}\".\n\
             Output:\n\
             1. Whether the element is currently visible.\n\
             2. Its exact center coordinate as percentages from top-left: x (0.0 to 100.0) and y (0.0 to 100.0).\n\
             3. If not visible, describe what is currently visible on screen and recommend what action to take (e.g., scroll down, open menu, wait)."
        ),
        None => format!(
            "You are a mobile UI grounding engine for Android automation. \
             Carefully examine this live Android phone screen.\n\
             List all interactive UI elements currently visible (buttons, inputs, icons, tabs, list items).\n\
             For each element, provide: [Name/Label] at center coordinates (x: 0-100%, y: 0-100%).\n\
             Also report the active app or screen state and whether any keyboard/dialog is open."
        ),
    };

    let mut payload = json!({
        "model": model_name,
        "messages": [
            {
                "role": "system",
                "content": "You are Vivy Mobile Vision Engine. You analyze smartphone screen mirrors with pinpoint coordinate accuracy (0.0 to 100.0% normalized screen coordinates) so an automation agent can tap buttons without missing."
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "text",
                        "text": user_prompt
                    },
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": image_data_url
                        }
                    }
                ]
            }
        ],
        "temperature": 0.1,
        "max_tokens": 1024
    });
    creds.fit_request(&mut payload);

    let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let resp = client
        .post(&endpoint)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Vision request failed: {e}"))?;

    if !resp.status().is_success() {
        let err_text = resp.text().await.unwrap_or_default();
        return Err(format!("Vision API returned error: {err_text}"));
    }

    let json_resp: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse Vision API response: {e}"))?;

    let content = json_resp["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| "Empty vision analysis response from model".to_string())?;

    Ok(content.to_string())
}
