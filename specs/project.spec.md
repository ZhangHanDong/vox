spec: project
name: "Vox"
tags: [makepad, macos, voice, input-method, rust]
---

## 意图

Vox 是基于 Makepad 2.0 框架构建的 macOS 菜单栏语音输入法。用户按住 Option 键录音，松开后通过 ominix-api (Qwen3-ASR) 转录语音为文字，可选 LLM refine（纠错/翻译/文风转换），最终将文字注入当前聚焦的输入框。

## 约束

- 使用 Makepad 2.0 `script_mod!` + Splash DSL 语法，不使用 1.x `live_design!`
- Rust edition 2021（Makepad 兼容性要求）
- macOS FFI 封装为独立 `macos-sys` crate，使用 `makepad_objc_sys`（与 Makepad 共享 NSApplication 实例）
- 音频回调线程禁止分配内存、加锁或阻塞；使用 `Arc<AtomicU64>` 传递 RMS
- 跨线程通信模式：macos-sys → crossbeam channel → Makepad timer poll (10ms)
- 所有 ObjC 细节隐藏在 macos-sys 纯 Rust 接口后
- `#![warn(clippy::all)]` 在所有 lib.rs / main.rs
- Splash 中不能添加 `instance` 变量到已有 draw type（frozen vec 限制）

## 已定决策

- UI 框架: Makepad 2.0
- ASR: Qwen3-ASR via ominix-api HTTP (`POST /v1/audio/transcriptions`, JSON + base64)
- ASR language: 发送全名 ("Chinese", "Japanese", "English")，不是 ISO code
- LLM Refine: 任意 OpenAI 兼容端点 (`POST /v1/chat/completions`)
- LLM prompt: 包含目标语言标记 `[目标语言:xxx]`，支持纠错+翻译+文风转换
- 全局热键: 按住左 Option (CGEventFlags 0x080000)，CGEvent tap 不调用 CGEventTapEnable
- 悬浮窗: 透明窗口 (`window.transparent: true` + `pass.clear_color: #x00000000`)
- 胶囊形状: 自定义 SDF capsule shader（`clamp(px, r, w-r)` 算法）
- 菜单栏: NSStatusBar via `makepad_objc_sys`，全局单例 target + sender.tag()
- 隐藏 Dock: 不调用 `show_in_dock(false)`，.app bundle 用 `LSUIElement=true`
- 文字注入: 剪贴板 + 模拟 Cmd+V，注入前切换 CJK 输入法到 ASCII
- 配置存储: `~/.config/vox/config.json`，支持环境变量覆盖
- 默认 API 端口: 18080
- 默认语言: 简体中文 (zh)
- 支持语言: zh, en, zh-TW, ja, ko, wen (文言文)

## 边界

### 允许修改
- macos-sys/**
- app/**
- Cargo.toml
- specs/**

### 禁止做
- 不要修改 Makepad 框架源码
- 不要修改 OminiX-API 源码
- 不要在音频回调中使用 `Mutex::lock`（只允许 `try_lock`）
- 不要在 macos-sys crate 中引入 makepad 依赖
- 不要调用 `show_in_dock(false)`（会隐藏 NSStatusItem）
- 不要在 Splash `+:` 块中添加 `instance` 变量（frozen vec）

## 排除范围

- Windows / Linux / Web 平台支持（Roadmap v0.3）
- 自定义 ASR 模型训练
- 应用内模型下载管理
- iOS / Android 移植
- 流式转录（Roadmap v0.5）
