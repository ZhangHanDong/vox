use makepad_widgets::*;

/// Request ID for transcription HTTP calls.
pub const TRANSCRIBE_REQUEST_ID: LiveId = live_id!(transcribe);

/// Send a transcription request to ominix-api.
///
/// The API expects JSON with base64-encoded audio:
/// {"file": "<base64 WAV>", "language": "zh"}
pub fn send_transcribe_request(cx: &mut Cx, base_url: &str, wav_data: &[u8], language: &str) {
    let url = format!("{}/v1/audio/transcriptions", base_url.trim_end_matches('/'));

    // Base64 encode the WAV data
    let b64 = base64_encode(wav_data);

    // Build JSON body
    let body = format!(
        r#"{{"file":"{}","language":"{}","model":"qwen3-asr"}}"#,
        b64, language
    );

    let mut req = HttpRequest::new(url, HttpMethod::POST);
    req.set_header("Content-Type".into(), "application/json".into());
    req.set_body(body.into_bytes());

    cx.http_request(TRANSCRIBE_REQUEST_ID, req);
}

/// Parse the transcription response.
/// Expected format: {"text": "transcribed text"}
pub fn parse_transcribe_response(response: &HttpResponse) -> Result<String, String> {
    if response.status_code != 200 {
        return Err(format!("HTTP {}", response.status_code));
    }

    let body_str = response
        .body_string()
        .ok_or_else(|| "Empty response body".to_string())?;

    // Extract "text" field from JSON
    if let Some(start) = body_str.find("\"text\"") {
        let after_key = &body_str[start + 6..];
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

    Err(format!("Unexpected response format: {body_str}"))
}

/// Simple base64 encoder (no external deps).
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}
