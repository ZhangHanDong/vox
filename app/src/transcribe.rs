use makepad_widgets::*;
use serde_json::Value as JsonValue;

/// Request ID for OminiX-style single-shot transcription.
pub const TRANSCRIBE_REQUEST_ID: LiveId = live_id!(transcribe);

/// Streaming ASR: matches Python `QwenStreamingASRClient` (start → chunk → finish).
pub const ASR_STREAM_START_ID: LiveId = live_id!(asr_stream_start);
pub const ASR_STREAM_CHUNK_ID: LiveId = live_id!(asr_stream_chunk);
pub const ASR_STREAM_FINISH_ID: LiveId = live_id!(asr_stream_finish);

/// Settings "Test connection" for streaming ASR (start → minimal chunk → finish).
pub const TEST_ASR_STREAM_START_ID: LiveId = live_id!(test_asr_stream_start);
pub const TEST_ASR_STREAM_CHUNK_ID: LiveId = live_id!(test_asr_stream_chunk);
pub const TEST_ASR_STREAM_FINISH_ID: LiveId = live_id!(test_asr_stream_finish);

/// Send a transcription request to ominix-api (JSON + base64 WAV).
pub fn send_transcribe_request(
    cx: &mut Cx,
    base_url: &str,
    wav_data: &[u8],
    language: &str,
    model: &str,
) {
    let url = format!("{}/v1/audio/transcriptions", base_url.trim_end_matches('/'));

    // Qwen3-ASR expects full language names, not ISO codes
    let asr_language = match language {
        "zh" => "Chinese",
        "en" => "English",
        "ja" => "Japanese",
        "ko" => "Korean",
        "zh-TW" => "Chinese",
        "wen" => "Chinese", // 文言文：用中文 ASR 识别白话，LLM 转文言
        _ => language,
    };

    let b64 = base64_encode(wav_data);
    let body = serde_json::json!({
        "file": b64,
        "language": asr_language,
        "model": model,
    })
    .to_string();

    let mut req = HttpRequest::new(url, HttpMethod::POST);
    req.set_header("Content-Type".into(), "application/json".into());
    req.set_body(body.into_bytes());

    cx.http_request(TRANSCRIBE_REQUEST_ID, req);
}

/// Map app language to ISO code for `/api/start` (matches typical `ASR_LANGUAGE` env usage).
pub fn streaming_language_code(language: &str) -> &'static str {
    match language {
        "zh" | "wen" => "zh",
        "en" => "en",
        "zh-TW" => "zh-TW",
        "ja" => "ja",
        "ko" => "ko",
        _ => "zh",
    }
}

/// `POST {base}/api/start` — JSON `sample_rate`, `language`, `task`.
pub fn send_streaming_start(cx: &mut Cx, request_id: LiveId, base_url: &str, language: &str) {
    let url = format!("{}/api/start", base_url.trim_end_matches('/'));
    let lang = streaming_language_code(language);
    let body = serde_json::json!({
        "sample_rate": 16000_i32,
        "language": lang,
        "task": "transcribe",
    })
    .to_string();

    let mut req = HttpRequest::new(url, HttpMethod::POST);
    req.set_header("Content-Type".into(), "application/json".into());
    req.set_body(body.into_bytes());
    cx.http_request(request_id, req);
}

/// `POST {base}/api/chunk?session_id=...` — raw mono float32 little-endian PCM at 16 kHz.
pub fn send_streaming_chunk(
    cx: &mut Cx,
    request_id: LiveId,
    base_url: &str,
    session_id: &str,
    pcm: &[u8],
) {
    let url = format!(
        "{}/api/chunk?session_id={}",
        base_url.trim_end_matches('/'),
        query_escape(session_id)
    );
    let mut req = HttpRequest::new(url, HttpMethod::POST);
    req.set_header("Content-Type".into(), "application/octet-stream".into());
    req.set_body(pcm.to_vec());
    cx.http_request(request_id, req);
}

/// `POST {base}/api/finish?session_id=...`
pub fn send_streaming_finish(cx: &mut Cx, request_id: LiveId, base_url: &str, session_id: &str) {
    let url = format!(
        "{}/api/finish?session_id={}",
        base_url.trim_end_matches('/'),
        query_escape(session_id)
    );
    let req = HttpRequest::new(url, HttpMethod::POST);
    cx.http_request(request_id, req);
}

/// Parse `{"session_id":"..."}` from `/api/start`.
pub fn parse_stream_start_response(response: &HttpResponse) -> Result<String, String> {
    if response.status_code != 200 {
        return Err(format!("HTTP {}", response.status_code));
    }
    let body_str = response
        .body_string()
        .ok_or_else(|| "Empty response body".to_string())?;
    let v: JsonValue =
        serde_json::from_str(&body_str).map_err(|e| format!("Invalid JSON: {e}"))?;
    v.get("session_id")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("Missing session_id: {body_str}"))
}

/// Extract transcript text from streaming chunk/finish payloads (Python `_extract_text`).
pub fn parse_streaming_asr_text(response: &HttpResponse) -> Result<String, String> {
    if response.status_code != 200 {
        return Err(format!("HTTP {}", response.status_code));
    }
    let body_str = response
        .body_string()
        .ok_or_else(|| "Empty response body".to_string())?;
    extract_streaming_text(&body_str)
}

fn extract_streaming_text(body_str: &str) -> Result<String, String> {
    let v: JsonValue =
        serde_json::from_str(body_str).map_err(|e| format!("Invalid JSON: {e}"))?;
    if let JsonValue::String(s) = &v {
        return Ok(s.trim().to_string());
    }
    if let JsonValue::Object(map) = &v {
        for key in ["text", "result", "partial", "transcript"] {
            if let Some(JsonValue::String(s)) = map.get(key) {
                let t = s.trim();
                if !t.is_empty() {
                    return Ok(t.to_string());
                }
            }
        }
        if let Some(JsonValue::Object(nested)) = map.get("response") {
            if let Some(JsonValue::String(s)) = nested.get("text") {
                return Ok(s.trim().to_string());
            }
        }
    }
    Ok(String::new())
}

fn query_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// Parse the OminiX transcription response (`{"text": "..."}`).
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

/// Minimal float32 silence for connectivity test (~0.2 s at 16 kHz mono; f32le = 4 bytes/sample).
pub fn test_stream_silence_pcm() -> &'static [u8] {
    &[0u8; 3200 * 4]
}

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
