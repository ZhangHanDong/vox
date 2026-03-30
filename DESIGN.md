# Makepad Voice Input — Design Spec

> **Date:** 2026-03-30
> **Status:** Approved

## Overview

macOS 菜单栏语音输入法应用，使用 Makepad 2.0 框架 + Rust 实现。按住 Option 键录音，松开后通过 Qwen3-ASR (ominix-api) 转录，可选 LLM refine，最终将文字注入当前聚焦的输入框。

## 外部依赖源码路径

供 AI agent 查阅实现细节时使用：

| 依赖 | 本地路径 | 说明 |
|------|----------|------|
| **Makepad 2.0** | `/Users/zhangalex/Work/Projects/FW/robius/makepad/` | UI 框架，含 widgets、platform、draw 等 |
| **OminiX-MLX** | `/Users/zhangalex/Work/Projects/FW/robius/OminiX-MLX/` | ML 推理平台，含 ASR/TTS/LLM 模型 + ominix-api 服务 |

### Makepad 关键参考文件

- `examples/floating_panel/src/main.rs` — 无边框浮动面板窗口模式
- `widgets/src/window_voice_input.rs` — 音频录制 + RMS + 语音转录完整实现
- `widgets/src/window.rs` — Window widget，`ScriptWindowHandle`，`MacosWindowConfig`
- `platform/src/window.rs` — `MacosWindowKind`/`MacosWindowChrome`/`MacosWindowLevel` 枚举
- `platform/src/audio.rs` — `AudioBuffer`、`AudioInfo` 类型定义
- `platform/src/media_api.rs` — `CxMediaApi` trait (`audio_input`/`audio_output`)
- `platform/src/cx_api.rs` — `show_in_dock()`、`copy_to_clipboard()`、`http_request()`
- `platform/network/src/types.rs` — `HttpRequest`、`HttpResponse`、`HttpMethod`
- `platform/src/os/apple/macos/macos_app.rs` — macOS 平台层（剪贴板、窗口管理）
- `examples/teamtalk/src/main.rs` — P2P 音频聊天，`audio_input`/`audio_output` 完整示例

### OminiX-MLX 关键参考文件

- `ominix-api/README.md` — 统一 REST API 文档（OpenAI 兼容端点）
- `ominix-api/src/main.rs` — API 服务入口，端点路由
- `qwen3-asr-mlx/src/lib.rs` — Qwen3-ASR 公开 API（`transcribe_samples` 等）
- `qwen3-asr-mlx/README.md` — ASR 模型说明、性能基准、语言列表

## 设计决策记录

| 决策 | 选择 | 原因 |
|------|------|------|
| 语音识别 | Qwen3-ASR via ominix-api HTTP | 中文 CER 5.88 远超 Whisper，纯 Rust，未来可远程部署 |
| ASR 集成方式 | HTTP 调用 ominix-api | 服务独立管理，可复用，可远程 |
| 触发方式 | 按住左 Option 键，支持用户自定义 | 兼容性好，CGEvent tap 可控 |
| LLM Refine | 保留，通过 ominix-api LLM 端点 | 作为可选增强，复用同一服务 |
| macOS FFI | 独立 crate (macos-sys) | 解耦，可复用，接口清晰 |

## 架构

```
┌─────────────────────────────────────────────────┐
│                  Makepad App                     │
│                                                  │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │ Main     │  │ Capsule  │  │ Settings      │  │
│  │ Window   │  │ Float    │  │ Window        │  │
│  │ (hidden) │  │ Window   │  │               │  │
│  └──────────┘  └──────────┘  └───────────────┘  │
│       │              ▲              ▲             │
│       │         show/hide      open/close        │
│       ▼              │              │             │
│  ┌─────────────────────────────────────────────┐ │
│  │          App State & Event Loop              │ │
│  │  - audio capture (cx.audio_input)            │ │
│  │  - RMS → shader (AtomicU64)                  │ │
│  │  - HTTP → ominix-api (transcribe + refine)   │ │
│  │  - config persistence (~/.config/...)        │ │
│  └──────────┬──────────────────────┬────────────┘ │
│             │                      │              │
│             ▼                      ▼              │
│  ┌──────────────────┐  ┌─────────────────────┐   │
│  │   macos-sys       │  │   ominix-api        │   │
│  │   (FFI crate)     │  │   (HTTP service)    │   │
│  └──────────────────┘  └─────────────────────┘   │
└─────────────────────────────────────────────────┘
```

## 项目结构

```
makepad-voice-input/
├── Cargo.toml              # workspace
├── DESIGN.md               # this file
├── macos-sys/              # 独立 FFI crate
│   ├── Cargo.toml          # deps: objc2, core-foundation, block2
│   └── src/
│       ├── lib.rs          # pub mod + #[cfg(target_os = "macos")]
│       ├── event_tap.rs    # CGEvent tap — 全局热键监听
│       ├── status_bar.rs   # NSStatusBar + NSMenu
│       ├── clipboard.rs    # NSPasteboard 读写（含读取）
│       ├── input_source.rs # TIS 输入法检测/切换
│       └── key_inject.rs   # CGEventPost 模拟 Cmd+V
├── app/                    # Makepad 应用 crate
│   ├── Cargo.toml          # deps: makepad-widgets, macos-sys, serde, serde_json
│   └── src/
│       ├── main.rs         # app_main!, script_mod!, App struct
│       ├── audio.rs        # 音频录制 + RMS + WAV 编码
│       ├── transcribe.rs   # HTTP 调用 ominix-api /v1/audio/transcriptions
│       ├── llm_refine.rs   # HTTP 调用 ominix-api /v1/chat/completions
│       ├── text_inject.rs  # 编排：剪贴板→输入法切换→Cmd+V→恢复
│       └── config.rs       # 配置持久化
```

## 1. macos-sys FFI Crate

纯 Rust 接口，零 Makepad 依赖，隐藏全部 ObjC 细节。

### event_tap.rs — 全局热键

```rust
pub struct HotkeyConfig {
    pub key: HotkeyKey,
    pub trigger: HotkeyTrigger,
}

pub enum HotkeyKey {
    OptionLeft,
    OptionRight,
    FnKey,
    ControlLeft,
    ControlRight,
}

pub enum HotkeyTrigger {
    Hold,
    DoubleTap { interval_ms: u32 },
}

pub enum HotkeyEvent {
    Pressed,
    Released,
}

/// 启动全局热键监听。在独立线程上运行 CFRunLoop。
/// callback 在 CFRunLoop 线程上调用。
pub fn start_hotkey_monitor(
    config: HotkeyConfig,
    callback: impl Fn(HotkeyEvent) + Send + 'static,
) -> Result<HotkeyHandle, Error>;

pub fn stop_hotkey_monitor(handle: HotkeyHandle);
```

实现要点：
- `CGEventTapCreate` 监听 `kCGEventFlagsChanged`
- 检测 Option 键的 flag 位 (`NX_DEVICELALTKEYMASK`)
- 抑制事件传递（callback 返回 NULL）防止触发系统行为
- 独立线程运行 `CFRunLoopRun`，通过 `CFRunLoopStop` 停止

### status_bar.rs — 菜单栏图标

```rust
pub struct MenuItem {
    pub title: String,
    pub action_id: u64,
    pub enabled: bool,
    pub checked: bool,
    pub submenu: Option<Vec<MenuItem>>,
}

pub struct StatusBarHandle { /* opaque */ }

/// 创建菜单栏图标。返回 handle + Receiver 接收菜单点击事件。
pub fn create_status_bar(
    icon_data: &[u8],
    menu: Vec<MenuItem>,
) -> (StatusBarHandle, crossbeam_channel::Receiver<u64>);

pub fn update_menu(handle: &StatusBarHandle, menu: Vec<MenuItem>);
pub fn set_status_bar_icon(handle: &StatusBarHandle, icon_data: &[u8]);
pub fn remove_status_bar(handle: StatusBarHandle);
```

### clipboard.rs — 剪贴板

```rust
/// 读取剪贴板当前文本内容
pub fn read_clipboard() -> Option<String>;

/// 写入文本到剪贴板
pub fn write_clipboard(text: &str);
```

### input_source.rs — 输入法

```rust
/// 获取当前输入法 ID (如 "com.apple.inputmethod.SCIM.ITABC")
pub fn current_input_source_id() -> String;

/// 判断当前输入法是否为 CJK 输入法
pub fn is_cjk_input_source() -> bool;

/// 切换到 ASCII 输入源 (ABC/US 键盘)
pub fn select_ascii_input_source() -> Result<(), Error>;

/// 切换到指定输入法
pub fn select_input_source(id: &str) -> Result<(), Error>;
```

### key_inject.rs — 按键模拟

```rust
/// 模拟 Cmd+V 粘贴
pub fn simulate_cmd_v();
```

## 2. 悬浮窗 UI (Makepad)

### 窗口配置

第二个 Window，Startup 时配置为 floating panel：

```rust
// In App::handle_event, Event::Startup:
let panel = self.ui.window(cx, ids!(capsule_window));
let mut macos = MacosWindowConfig::floating_panel();
macos.chrome = MacosWindowChrome::Borderless;
macos.level = MacosWindowLevel::Floating;
macos.non_activating = true;
macos.join_all_spaces = true;
macos.becomes_key_only_if_needed = true;
panel.configure_macos_window(cx, macos);
```

### 胶囊 UI (Splash DSL)

```
capsule_window := Window{
    show_caption_bar: false
    window.inner_size: vec2(240, 56)
    window.transparent: true
    window.backdrop: Vibrancy
    body +: {
        View{
            width: Fill height: Fill
            flow: Overlay
            align: Align{x: 0.5 y: 1.0}
            padding: Inset{bottom: 80}

            capsule := RoundedView{
                width: Fit height: 56
                padding: Inset{left: 12 right: 16 top: 12 bottom: 12}
                flow: Right spacing: 10
                align: Align{y: 0.5}
                draw_bg +: {
                    instance opacity: 0.0
                    instance scale: 0.0
                    color: #x1a1a2e80
                    border_radius: 28.0
                }
                new_batch: true
                animator: Animator{
                    show: {
                        default: @hide
                        hide: AnimatorState{
                            from: {all: Forward {duration: 0.22}}
                            ease: InCubic
                            apply: {draw_bg: {opacity: 0.0 scale: 0.0}}
                        }
                        on: AnimatorState{
                            from: {all: Forward {duration: 0.35}}
                            ease: OutElastic
                            apply: {draw_bg: {opacity: 1.0 scale: 1.0}}
                        }
                    }
                }

                // 波形 — 自定义 shader, 44x32
                waveform := View{
                    width: 44 height: 32
                    draw_bg +: {
                        instance bar0: 0.15
                        instance bar1: 0.15
                        instance bar2: 0.15
                        instance bar3: 0.15
                        instance bar4: 0.15

                        pixel: fn() {
                            let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                            let w = self.rect_size.x
                            let h = self.rect_size.y
                            let bar_w = 3.0
                            let gap = 3.5
                            let total = bar_w * 5.0 + gap * 4.0
                            let sx = (w - total) * 0.5
                            let bars = [self.bar0, self.bar1, self.bar2, self.bar3, self.bar4]
                            let cy = h * 0.5

                            // bar 0
                            let bh = max(4.0, self.bar0 * h * 0.9)
                            sdf.box(sx, cy - bh * 0.5, bar_w, bh, 1.5)
                            sdf.fill(#xffffffcc)

                            // bar 1
                            let bh1 = max(4.0, self.bar1 * h * 0.9)
                            sdf.box(sx + bar_w + gap, cy - bh1 * 0.5, bar_w, bh1, 1.5)
                            sdf.fill(#xffffffcc)

                            // bar 2
                            let bh2 = max(4.0, self.bar2 * h * 0.9)
                            sdf.box(sx + 2.0 * (bar_w + gap), cy - bh2 * 0.5, bar_w, bh2, 1.5)
                            sdf.fill(#xffffffcc)

                            // bar 3
                            let bh3 = max(4.0, self.bar3 * h * 0.9)
                            sdf.box(sx + 3.0 * (bar_w + gap), cy - bh3 * 0.5, bar_w, bh3, 1.5)
                            sdf.fill(#xffffffcc)

                            // bar 4
                            let bh4 = max(4.0, self.bar4 * h * 0.9)
                            sdf.box(sx + 4.0 * (bar_w + gap), cy - bh4 * 0.5, bar_w, bh4, 1.5)
                            sdf.fill(#xffffffcc)

                            return sdf.result
                        }
                    }
                }

                // 转录文字
                transcript_label := Label{
                    width: Fit
                    text: "Listening..."
                    draw_text.color: #xffffffdd
                    draw_text.text_style.font_size: 14
                }
            }
        }
    }
}
```

### 波形驱动

```rust
// Audio thread → UI thread via atomics
let rms_atomic = Arc::new(AtomicU64::new(0u64));

cx.audio_input(0, move |_info, buffer| {
    let mut sum = 0.0f32;
    let ch = buffer.channel(0);
    for &s in ch { sum += s * s; }
    let rms = (sum / ch.len() as f32).sqrt();
    rms_atomic.store(rms.to_bits() as u64, Ordering::Relaxed);
});

// In handle_next_frame:
let raw_rms = f32::from_bits(self.rms_atomic.load(Ordering::Relaxed) as u32);
// Smoothing: attack 40%, release 15%
let alpha = if raw_rms > self.smooth_rms { 0.4 } else { 0.15 };
self.smooth_rms += (raw_rms - self.smooth_rms) * alpha;

// Apply per-bar weights + jitter
let weights = [0.5, 0.8, 1.0, 0.75, 0.55];
let bars: [f64; 5] = weights.map(|w| {
    let jitter = 1.0 + (rand() * 0.08 - 0.04); // ±4%
    (self.smooth_rms * w * jitter * 8.0).clamp(0.05, 1.0) as f64
});

script_apply_eval!(cx, self.ui, {
    waveform.draw_bg.bar0: #(bars[0])
    waveform.draw_bg.bar1: #(bars[1])
    waveform.draw_bg.bar2: #(bars[2])
    waveform.draw_bg.bar3: #(bars[3])
    waveform.draw_bg.bar4: #(bars[4])
});
```

### 动画

| 动画 | 触发 | 实现 |
|------|------|------|
| 入场弹簧 | Option 按下 | Animator `show.on`，OutElastic 0.35s，scale 0→1 + opacity 0→1 |
| 退场缩放 | 注入完成 | Animator `show.hide`，InCubic 0.22s，scale 1→0 + opacity 1→0 |
| 波形 | 录音中 | NextFrame 循环，读 AtomicU64 RMS → script_apply_eval! |
| 文字宽度 | 转录更新 | Window resize via `capsule_window.resize(cx, new_size)` |

## 3. 数据流

```
[按住 Option]
    │ HotkeyEvent::Pressed (macos-sys → crossbeam channel)
    │ Makepad 主线程通过 SignalToUI 或 timer poll 收到信号
    ▼
cx.audio_input() 开始捕获
    │ 音频线程: 计算 RMS → Arc<AtomicU64>
    │ 音频线程: 累积 PCM samples → Arc<Mutex<Vec<f32>>>
    ▼
[NextFrame loop]
    │ 读 RMS → 平滑包络 → 更新 waveform shader instances
    │ 显示悬浮窗（定位到屏幕底部居中）+ 入场动画
    ▼
[松开 Option]
    │ HotkeyEvent::Released
    ▼
停止录音, 编码 WAV (16kHz mono)
    │
    ▼
HTTP POST → ominix-api /v1/audio/transcriptions
    │ multipart/form-data: file=audio.wav, language=zh
    │ 悬浮窗显示 "Transcribing..."
    ▼
收到转录文本 → 悬浮窗显示转录结果
    │
    ▼ (if LLM refine enabled)
HTTP POST → ominix-api /v1/chat/completions
    │ model: 配置的模型
    │ system: 保守纠错 prompt
    │ user: 转录文本
    │ 悬浮窗显示 "Refining..."
    ▼
收到最终文本
    │
    ▼
text_inject 编排:
    1. read_clipboard() → 保存原内容
    2. is_cjk_input_source()? → select_ascii_input_source()
    3. write_clipboard(&final_text)
    4. simulate_cmd_v()
    5. 50ms delay (cx.start_timeout)
    6. select_input_source(&original_id) → 恢复输入法
    7. write_clipboard(&original_content) → 恢复剪贴板
    │
    ▼
退场动画 → 隐藏悬浮窗
```

## 4. 配置

### 存储路径

`~/.config/makepad-voice-input/config.json`

### Schema

```json
{
    "language": "zh",
    "hotkey": {
        "key": "OptionLeft",
        "trigger": "Hold"
    },
    "ominix_api": {
        "base_url": "http://localhost:8080",
        "asr_model": "qwen3-asr"
    },
    "llm_refine": {
        "enabled": true,
        "api_base_url": "http://localhost:8080",
        "api_key": "",
        "model": "qwen3-4b",
        "system_prompt": "你是一个语音识别纠错助手。只修复明显的语音识别错误（如中文谐音错误、英文技术术语被错误转为中文，例如「配森」→「Python」、「杰森」→「JSON」）。绝对不要改写、润色或删除任何看起来正确的内容。如果输入看起来正确，必须原样返回。"
    }
}
```

## 5. 设置窗口 (Makepad)

第三个 Window（普通窗口，非 floating），内容：

```
settings_window := Window{
    window.title: "Voice Input Settings"
    window.inner_size: vec2(480, 520)
    body +: {
        ScrollYView{
            width: Fill height: Fill
            flow: Down spacing: 16 padding: 24

            // — ominix-api Section —
            Label{ text: "ominix-api" ... font_size: 16 }
            Label{ text: "Base URL" ... }
            api_url_input := TextInput{ empty_text: "http://localhost:8080" }

            Hr{}

            // — Language —
            Label{ text: "Language" ... }
            language_dropdown := DropDown{
                labels: ["简体中文", "English", "繁體中文", "日本語", "한국어"]
            }

            // — Hotkey —
            Label{ text: "Hotkey" ... }
            hotkey_dropdown := DropDown{
                labels: ["Hold Left Option", "Hold Right Option",
                         "Hold Left Control", "Hold Fn"]
            }

            Hr{}

            // — LLM Refine Section —
            View{ flow: Right align: Align{y: 0.5}
                Label{ text: "LLM Refinement" ... }
                Filler{}
                llm_toggle := Toggle{}
            }
            Label{ text: "API Base URL" ... }
            llm_url_input := TextInput{ empty_text: "http://localhost:8080" }
            Label{ text: "API Key" ... }
            llm_key_input := TextInput{ empty_text: "sk-..." password: true }
            Label{ text: "Model" ... }
            llm_model_input := TextInput{ empty_text: "qwen3-4b" }

            Hr{}

            // — Actions —
            View{ flow: Right spacing: 8 align: Align{x: 1.0}
                test_button := Button{ text: "Test Connection" }
                save_button := Button{ text: "Save" }
            }

            status_label := Label{ text: "" draw_text.color: #x88cc88 }
        }
    }
}
```

## 6. 菜单栏 (macos-sys)

通过 NSStatusBar FFI 创建，非 Makepad 窗口。

### 菜单结构

```
[🎤] (麦克风图标，录音时变红点)
├── Language ▸
│   ├── ✓ 简体中文
│   ├── English
│   ├── 繁體中文
│   ├── 日本語
│   └── 한국어
├── ─────────
├── LLM Refinement ▸
│   ├── ✓ Enabled / Disabled
│   └── Settings...       → 打开设置窗口
├── ─────────
└── Quit
```

### 菜单事件

菜单点击 → crossbeam channel → Makepad 主线程 timer poll 读取 → 执行对应操作。

```rust
// In handle_timer (10ms interval poll):
while let Ok(action_id) = self.menu_rx.try_recv() {
    match action_id {
        MENU_LANG_ZH => self.set_language(cx, "zh"),
        MENU_LANG_EN => self.set_language(cx, "en"),
        MENU_LLM_TOGGLE => self.toggle_llm_refine(cx),
        MENU_SETTINGS => self.show_settings_window(cx),
        MENU_QUIT => cx.quit(),
        _ => {}
    }
}
```

## 7. 应用生命周期

```rust
impl AppMain for App {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event) {
        match event {
            Event::Startup => {
                // 1. 隐藏 Dock 图标
                cx.show_in_dock(false);
                // 2. 配置悬浮窗为 floating panel
                self.configure_capsule_window(cx);
                // 3. 加载配置
                self.load_config();
                // 4. 创建菜单栏图标 (macos-sys)
                self.setup_status_bar();
                // 5. 启动全局热键监听 (macos-sys)
                self.setup_hotkey_monitor();
                // 6. 启动菜单事件 poll timer
                self.menu_poll_timer = cx.start_interval(0.01);
                // 7. 隐藏主窗口和悬浮窗
                self.ui.window(cx, ids!(main_window)).minimize(cx);
            }
            Event::Shutdown => {
                self.cleanup();
            }
            _ => {}
        }
        self.match_event(cx, event);
        self.ui.handle_event(cx, event, &mut Scope::empty());
    }
}
```

## 8. 依赖

### macos-sys/Cargo.toml

```toml
[package]
name = "macos-sys"
version = "0.1.0"
edition = "2024"

[dependencies]
crossbeam-channel = "0.5"
thiserror = "2"

[target.'cfg(target_os = "macos")'.dependencies]
core-foundation = "0.10"
core-graphics = "0.24"
objc2 = "0.6"
objc2-foundation = { version = "0.3", features = ["NSString", "NSArray"] }
objc2-app-kit = { version = "0.3", features = [
    "NSStatusBar", "NSStatusItem", "NSMenu", "NSMenuItem",
    "NSPasteboard", "NSImage", "NSEvent"
] }
block2 = "0.6"
```

### app/Cargo.toml

```toml
[package]
name = "makepad-voice-input"
version = "0.1.0"
edition = "2024"

[dependencies]
makepad-widgets = { path = "../../FW/robius/makepad/widgets" }
macos-sys = { path = "../macos-sys" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

## 9. 实现阶段

### Phase 1: 骨架 + 悬浮窗

- Workspace + 两个 crate 骨架
- Makepad app 启动，隐藏 Dock
- 悬浮窗 UI（静态胶囊 + 波形 shader）
- macos-sys: status_bar 最小实现（图标 + Quit）

### Phase 2: 音频 + 热键

- macos-sys: event_tap (Option 键监听)
- cx.audio_input() 录音 + RMS 计算
- 波形动画（NextFrame + AtomicU64）
- WAV 编码

### Phase 3: 转录 + 注入

- HTTP 调用 ominix-api 转录
- macos-sys: clipboard 读写 + input_source + key_inject
- text_inject 完整编排
- 悬浮窗状态显示（Listening → Transcribing → 文字）

### Phase 4: LLM Refine + 设置

- LLM refine HTTP 调用
- 设置窗口 UI + 配置持久化
- 菜单栏完整菜单（语言切换、LLM 开关）
- Test Connection 功能

### Phase 5: 打磨

- 入场/退场动画
- 文字宽度弹性过渡
- 录音时菜单栏图标变红
- 错误处理和边界情况
- .app bundle 打包
