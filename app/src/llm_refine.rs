use makepad_widgets::*;

pub const LLM_REFINE_REQUEST_ID: LiveId = live_id!(llm_refine);

/// Send an LLM refine request to correct transcription errors.
pub fn send_refine_request(
    cx: &mut Cx,
    api_base_url: &str,
    api_key: &str,
    model: &str,
    system_prompt: &str,
    text: &str,
) {
    let url = format!(
        "{}/v1/chat/completions",
        api_base_url.trim_end_matches('/')
    );

    let body = format!(
        r#"{{"model":"{}","messages":[{{"role":"system","content":{}}},{{"role":"user","content":{}}}],"temperature":0.1,"max_tokens":2048}}"#,
        model,
        serde_json::to_string(system_prompt).unwrap_or_default(),
        serde_json::to_string(text).unwrap_or_default(),
    );

    let mut req = HttpRequest::new(url, HttpMethod::POST);
    req.set_header("Content-Type".into(), "application/json".into());
    if !api_key.is_empty() {
        req.set_header("Authorization".into(), format!("Bearer {api_key}"));
    }
    req.set_body(body.into_bytes());

    cx.http_request(LLM_REFINE_REQUEST_ID, req);
    log!("LLM refine request sent");
}

/// Parse the LLM refine response.
/// Expected OpenAI-compatible format: {"choices":[{"message":{"content":"..."}}]}
pub fn parse_refine_response(response: &HttpResponse) -> Result<String, String> {
    if response.status_code != 200 {
        return Err(format!("HTTP {}", response.status_code));
    }

    let body_str = response
        .body_string()
        .ok_or_else(|| "Empty response body".to_string())?;

    // Extract content from the first choice
    if let Some(content_start) = body_str.find("\"content\"") {
        let after_key = &body_str[content_start + 9..];
        let after_colon = after_key
            .trim_start()
            .strip_prefix(':')
            .unwrap_or(after_key)
            .trim_start();

        if let Some(stripped) = after_colon.strip_prefix('"') {
            let mut result = String::new();
            let mut chars = stripped.chars();
            while let Some(ch) = chars.next() {
                if ch == '\\' {
                    if let Some(escaped) = chars.next() {
                        match escaped {
                            'n' => result.push('\n'),
                            't' => result.push('\t'),
                            '"' => result.push('"'),
                            '\\' => result.push('\\'),
                            _ => {
                                result.push('\\');
                                result.push(escaped);
                            }
                        }
                    }
                } else if ch == '"' {
                    break;
                } else {
                    result.push(ch);
                }
            }
            return Ok(result);
        }
    }

    Err(format!("Unexpected LLM response format: {body_str}"))
}
