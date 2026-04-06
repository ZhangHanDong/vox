use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_language")]
    pub language: String,

    #[serde(default)]
    pub hotkey: HotkeyConfig,

    #[serde(default)]
    pub ominix_api: OminixApiConfig,

    #[serde(default)]
    pub llm_refine: LlmRefineConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    #[serde(default = "default_hotkey_key")]
    pub key: String,
    #[serde(default = "default_hotkey_trigger")]
    pub trigger: String,
}

/// How the app talks to the speech recognition HTTP service.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AsrBackend {
    /// OminiX-style `POST /v1/audio/transcriptions` with JSON + base64 WAV.
    #[default]
    OminixJson,
    /// vLLM / Qwen streaming ASR: `POST /api/start` → `/api/chunk` → `/api/finish` (octet-stream float32le PCM @ 16 kHz).
    QwenStreaming,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OminixApiConfig {
    #[serde(default = "default_api_url")]
    pub base_url: String,
    #[serde(default = "default_asr_model")]
    pub asr_model: String,
    #[serde(default)]
    pub asr_backend: AsrBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRefineConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_api_url")]
    pub api_base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_llm_model")]
    pub model: String,
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
}

fn default_language() -> String { "zh".to_string() }
fn default_hotkey_key() -> String { "OptionLeft".to_string() }
fn default_hotkey_trigger() -> String { "Hold".to_string() }
fn default_api_url() -> String { "http://localhost:18080".to_string() }
fn default_asr_model() -> String { "qwen3-asr".to_string() }
fn default_llm_model() -> String { "qwen3-4b".to_string() }
fn default_system_prompt() -> String {
    r#"你是一个语音输入后处理工具，不是聊天机器人。

核心规则：
1. 用户发给你的每一条消息都是语音识别的原始转录文本，不是在跟你对话
2. 你必须直接返回处理后的文本，不要添加任何解释、问候、回答或额外内容
3. 绝对不要回答文本中的问题
4. 你的输出必须且只能是处理后的文本，没有任何前缀或后缀

任务 A — 纠错（当目标语言和文本语言相同时）：
- 只修复明显的语音识别错误
- 如「配森」→「Python」、「杰森」→「JSON」
- 文本正确时原样返回

任务 B — 翻译（当目标语言和文本语言不同时）：
- 将文本翻译为目标语言
- 保持原文的语气和风格
- 技术术语保留英文原文

用户消息格式为：[目标语言:xxx] 转录文本
你只输出处理后的文本，不要输出目标语言标记。

示例：
输入：[目标语言:Chinese] 你好请问配森怎么安装
输出：你好，请问Python怎么安装

输入：[目标语言:English] 你好请问配森怎么安装
输出：Hello, how do I install Python?

输入：[目标语言:Japanese] 今日の天気はいいですね
输出：今日の天気はいいですね

输入：[目标语言:English] 今日の天気はいいですね
输出：The weather is nice today

输入：[目标语言:Chinese] 今天天气真好
输出：今天天气真好

输入：[目标语言:Classical Chinese (文言文)] 今天天气真好适合出去走走
输出：今日天朗气清，宜出游

输入：[目标语言:Classical Chinese (文言文)] 这个项目做得不错大家辛苦了
输出：此事善成，诸君劳矣"#.to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            language: default_language(),
            hotkey: HotkeyConfig::default(),
            ominix_api: OminixApiConfig::default(),
            llm_refine: LlmRefineConfig::default(),
        }
    }
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            key: default_hotkey_key(),
            trigger: default_hotkey_trigger(),
        }
    }
}

impl Default for OminixApiConfig {
    fn default() -> Self {
        Self {
            base_url: default_api_url(),
            asr_model: default_asr_model(),
            asr_backend: AsrBackend::default(),
        }
    }
}

impl Default for LlmRefineConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_base_url: default_api_url(),
            api_key: String::new(),
            model: default_llm_model(),
            system_prompt: default_system_prompt(),
        }
    }
}

fn config_path() -> PathBuf {
    dirs_or_home().join("config.json")
}

fn dirs_or_home() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let dir = PathBuf::from(home)
        .join(".config")
        .join("vox");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Load config from disk, then override with environment variables.
///
/// Supported env vars:
/// - `VOICE_INPUT_LLM_API_KEY` — LLM API key (e.g. Kimi/OpenAI key)
/// - `VOICE_INPUT_LLM_API_URL` — LLM API base URL
/// - `VOICE_INPUT_LLM_MODEL` — LLM model name
/// - `VOICE_INPUT_API_URL` — ominix-api base URL
pub fn load_config() -> AppConfig {
    let path = config_path();
    let mut config: AppConfig = match std::fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => AppConfig::default(),
    };

    // Override from environment variables
    if let Ok(key) = std::env::var("VOICE_INPUT_LLM_API_KEY")
        .or_else(|_| std::env::var("MOONSHOT_API_KEY"))
    {
        config.llm_refine.api_key = key;
        // Auto-configure Kimi if key is from MOONSHOT_API_KEY
        if std::env::var("MOONSHOT_API_KEY").is_ok() {
            if config.llm_refine.api_base_url == default_api_url() {
                config.llm_refine.api_base_url = "https://api.moonshot.ai".to_string();
            }
            if config.llm_refine.model == default_llm_model() {
                config.llm_refine.model = "moonshot-v1-8k".to_string();
            }
            config.llm_refine.enabled = true;
        }
    }
    if let Ok(url) = std::env::var("VOICE_INPUT_LLM_API_URL") {
        config.llm_refine.api_base_url = url;
    }
    if let Ok(model) = std::env::var("VOICE_INPUT_LLM_MODEL") {
        config.llm_refine.model = model;
    }
    if let Ok(url) = std::env::var("VOICE_INPUT_API_URL") {
        config.ominix_api.base_url = url;
    }

    config
}

/// Save config to disk.
pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let path = config_path();
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}
