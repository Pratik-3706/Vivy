use serde_json::{json, Value};

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": {
                "type": "object",
                "properties": properties,
                "required": required
            }
        }
    })
}

pub fn get_agent_tools_schema() -> Vec<Value> {
    vec![
        tool(
            "inspect_screen",
            "Read what is on the phone screen right now: the foreground app and a list of visible elements \
             (text, buttons, inputs) with their center coordinates in percent. Call this before acting and \
             again after every action that changes the screen.",
            json!({
                "target": {
                    "type": "string",
                    "description": "Optional: the element you are looking for, e.g. 'Search button'."
                },
                "visual": {
                    "type": "boolean",
                    "description": "Also analyse the screenshot visually. Slower. Use when the element list is missing what you need (images, games, video, maps)."
                }
            }),
            &[],
        ),
        tool(
            "tap_element",
            "Tap the on-screen element whose visible text or content description matches. Most reliable way to tap.",
            json!({
                "text": { "type": "string", "description": "Text or description of the element, as listed by inspect_screen." }
            }),
            &["text"],
        ),
        tool(
            "tap",
            "Tap at a screen position given in percent (0-100). (0,0) is the top-left corner.",
            json!({
                "x": { "type": "number", "description": "Percent of screen width from the left edge" },
                "y": { "type": "number", "description": "Percent of screen height from the top edge" },
                "element_label": { "type": "string", "description": "What you are tapping, for the activity log" }
            }),
            &["x", "y"],
        ),
        tool(
            "long_press",
            "Press and hold at a screen position given in percent.",
            json!({
                "x": { "type": "number", "description": "Percent of screen width" },
                "y": { "type": "number", "description": "Percent of screen height" },
                "duration_ms": { "type": "integer", "description": "Hold time in ms (default 800)" }
            }),
            &["x", "y"],
        ),
        tool(
            "swipe",
            "Swipe from one point to another (percent coordinates).",
            json!({
                "x1": { "type": "number" },
                "y1": { "type": "number" },
                "x2": { "type": "number" },
                "y2": { "type": "number" },
                "duration_ms": { "type": "integer", "description": "Default 300" }
            }),
            &["x1", "y1", "x2", "y2"],
        ),
        tool(
            "scroll",
            "Scroll the content. DOWN reveals content further down the page.",
            json!({
                "direction": { "type": "string", "enum": ["UP", "DOWN", "LEFT", "RIGHT"] }
            }),
            &["direction"],
        ),
        tool(
            "type_text",
            "Set the text of the focused input field (replaces its current content). Tap the field first.",
            json!({
                "text": { "type": "string" }
            }),
            &["text"],
        ),
        tool(
            "press_key",
            "Press a system key. ENTER submits the focused field (search, send).",
            json!({
                "key": {
                    "type": "string",
                    "enum": ["BACK", "HOME", "RECENTS", "ENTER", "NOTIFICATIONS", "VOLUME_UP", "VOLUME_DOWN"]
                }
            }),
            &["key"],
        ),
        tool(
            "open_app",
            "Launch an installed app by name (e.g. 'YouTube', 'Chrome', 'WhatsApp'). The phone matches the name \
             against installed apps, so the package name is optional.",
            json!({
                "app_name": { "type": "string", "description": "App name as shown on the home screen" },
                "package_name": { "type": "string", "description": "Optional Android package id, e.g. com.google.android.youtube" }
            }),
            &["app_name"],
        ),
        tool(
            "wait",
            "Wait for loading or animations to finish.",
            json!({
                "seconds": { "type": "number", "description": "0.5 to 10" }
            }),
            &["seconds"],
        ),
        tool(
            "web_search",
            "Search the web for information the task needs.",
            json!({
                "query": { "type": "string" }
            }),
            &["query"],
        ),
        tool(
            "ask_permission",
            "Ask the user on the PC for approval before touching private content (photos, files, messages, mails) \
             or before deleting anything or spending money.",
            json!({
                "category": {
                    "type": "string",
                    "enum": ["photos", "files", "messages", "mails", "destructive", "financial"]
                },
                "reason": { "type": "string", "description": "One sentence telling the user what you want to do and why" }
            }),
            &["category", "reason"],
        ),
        tool(
            "finish_task",
            "End the task. Call when the goal is done or cannot be done.",
            json!({
                "summary": { "type": "string", "description": "What was done, or why it could not be done" }
            }),
            &["summary"],
        ),
    ]
}
