spec: project
name: "Makepad Voice Input"
tags: [makepad, macos, voice, input-method]
---

## 意图

基于 Makepad 2.0 框架构建 macOS 菜单栏语音输入法。用户按住 Option 键录音，松开后通过 ominix-api (Qwen3-ASR) 转录语音为文字，可选 LLM refine 纠错，最终将文字注入当前聚焦的输入框。

## 约束

- 使用 Makepad 2.0 `script_mod!` + Splash DSL 语法，不使用 1.x `live_design!`
- Rust edition 2024
- macOS FFI 封装为独立 `macos-sys` crate，零 Makepad 依赖
- 音频回调线程禁止分配内存、加锁或阻塞；使用 `Arc<AtomicU64>` 传递 RMS
- 跨线程通信模式：macos-sys → crossbeam channel → Makepad timer poll
- 所有 ObjC 细节隐藏在 macos-sys 纯 Rust 接口后
- 不使用 `.unwrap()` 的生产路径；error 类型使用 `thiserror`(macos-sys) / `anyhow`(app)
- `#![warn(clippy::all)]` 在所有 lib.rs / main.rs

## 已定决策

- UI 框架: Makepad 2.0，源码路径 `/Users/zhangalex/Work/Projects/FW/robius/makepad/`
- ASR: Qwen3-ASR via ominix-api HTTP (`POST /v1/audio/transcriptions`)
- LLM Refine: ominix-api LLM 端点 (`POST /v1/chat/completions`)
- OminiX-MLX 源码路径: `/Users/zhangalex/Work/Projects/FW/robius/OminiX-MLX/`
- 全局热键: 默认按住左 Option，用户可自定义，通过 CGEvent tap 实现
- 悬浮窗: Makepad Window + `MacosWindowConfig::floating_panel()` + Borderless
- 菜单栏: NSStatusBar via macos-sys FFI
- 隐藏 Dock: `cx.show_in_dock(false)`
- 文字注入: 剪贴板 + 模拟 Cmd+V，注入前切换 CJK 输入法到 ASCII
- 配置存储: `~/.config/makepad-voice-input/config.json`
- 默认语言: 简体中文 (zh)

## 边界

### 允许修改
- macos-sys/**
- app/**
- Cargo.toml
- specs/**

### 禁止做
- 不要修改 Makepad 框架源码
- 不要修改 OminiX-MLX 源码
- 不要在音频回调中使用 `Mutex::lock`（只允许 `try_lock`）
- 不要在 macos-sys crate 中引入 makepad 依赖

## 排除范围

- Windows / Linux 平台支持
- 自定义 ASR 模型训练
- 应用内模型下载管理
- iOS / Android 移植
