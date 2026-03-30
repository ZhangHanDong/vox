# Implementation Plan

> Generated from specs/*.spec.md | 2026-03-30

## Dependency Graph

```
Phase 1: 骨架 + 悬浮窗 (1d)
    │
    ▼
Phase 2: 音频 + 热键 (2d)
    │
    ▼
Phase 3: 转录 + 注入 (2d)
    │
    ▼
Phase 4: LLM + 设置 (1.5d)
    │
    ▼
Phase 5: 打磨 + 打包 (1d)

Total estimated: ~7.5d
Critical path: all phases sequential
```

## Phase 1: 项目骨架 + 悬浮窗

**Spec:** `specs/phase1-skeleton.spec.md` | **Estimate:** 1d | **Scenarios:** 6

### Steps

1. **Workspace Cargo.toml** — 创建根 `Cargo.toml`，定义 workspace members
2. **macos-sys crate 骨架**
   - `macos-sys/Cargo.toml` — 依赖 objc2, core-foundation, crossbeam-channel, thiserror
   - `macos-sys/src/lib.rs` — pub mod 声明 + `#[cfg(target_os = "macos")]` guard
   - `macos-sys/src/status_bar.rs` — `create_status_bar()` + `remove_status_bar()` 最小实现
   - `macos-sys/src/event_tap.rs` — stub（`start_hotkey_monitor` 打印日志 + 返回 Ok）
3. **app crate 骨架**
   - `app/Cargo.toml` — 依赖 makepad-widgets (path), macos-sys (path), serde, serde_json, anyhow
   - `app/src/main.rs` — `app_main!`, `script_mod!`, App struct, MatchEvent, AppMain
4. **Splash UI 定义** — main_window (hidden) + capsule_window (floating panel)
   - 胶囊 RoundedView: 56px height, border_radius 28, 半透明背景
   - 波形 View: 44x32 with SDF2D shader (5 bars, static instance values)
   - transcript Label: "Listening..."
5. **Startup 逻辑**
   - `cx.show_in_dock(false)`
   - `configure_macos_window(cx, floating_panel + Borderless)`
   - 创建 NSStatusBar (macos-sys) 含 Quit 菜单
   - 10ms interval timer poll menu channel
6. **验证** — `cargo build --workspace` + `cargo clippy --workspace`

### Review Checkpoint

- [ ] `cargo build --workspace` 编译通过
- [ ] 运行后 Dock 无图标，菜单栏有图标
- [ ] 悬浮窗可手动触发显示，无标题栏

---

## Phase 2: 音频录制 + 全局热键

**Spec:** `specs/phase2-audio-hotkey.spec.md` | **Estimate:** 2d | **Scenarios:** 7

### Steps

1. **macos-sys: event_tap.rs 完整实现**
   - `CGEventTapCreate` 监听 `kCGEventFlagsChanged`
   - 检测左 Option flag (`NX_DEVICELALTKEYMASK`)
   - 独立线程 `CFRunLoopRun`，callback 通过 crossbeam channel 发送 `HotkeyEvent`
   - 抑制事件（返回 NULL）
   - 短按过滤（< 100ms 不触发）
   - 权限检测错误处理（`AXIsProcessTrusted`）
2. **app/src/audio.rs**
   - `AudioCapture` struct: `Arc<AtomicU64>` (RMS), `Arc<Mutex<Vec<f32>>>` (PCM)
   - `start(cx)`: 调用 `cx.audio_input(0, callback)`
   - callback: 计算 RMS → store AtomicU64, try_lock → push PCM
   - `stop() -> Vec<f32>`: 停止捕获，drain PCM buffer
   - `encode_wav(samples, sample_rate) -> Vec<u8>`: 编码 16kHz mono 16-bit PCM WAV
3. **波形动画**
   - `handle_next_frame`: 读 AtomicU64 RMS → 平滑包络 → 计算 bar0-bar4 → `script_apply_eval!`
   - 入场动画: Animator `show.on` (OutElastic 0.35s)
   - 退场动画: Animator `show.hide` (InCubic 0.22s)
4. **热键 → 录音 编排**
   - timer poll 收到 Pressed → start audio + show capsule + animate in
   - timer poll 收到 Released → stop audio + encode WAV + animate out
5. **验证** — 按住 Option 看到悬浮窗 + 波形动画，松开产出 WAV

### Review Checkpoint

- [ ] Option 按住显示悬浮窗，波形随声音变化
- [ ] 松开产出 WAV bytes（可保存文件验证）
- [ ] 音频回调用 `try_lock` 不阻塞

---

## Phase 3: 转录 + 文字注入

**Spec:** `specs/phase3-transcribe-inject.spec.md` | **Estimate:** 2d | **Scenarios:** 9

### Steps

1. **app/src/transcribe.rs**
   - `send_transcribe_request(cx, wav_bytes, language)`: 构建 multipart/form-data HttpRequest
   - 发送到 `{base_url}/v1/audio/transcriptions`
   - `handle_transcribe_response(response) -> Result<String>`: 解析 JSON response
2. **macos-sys: clipboard.rs**
   - `read_clipboard() -> Option<String>`: NSPasteboard `stringForType:`
   - `write_clipboard(text: &str)`: NSPasteboard `setString:forType:`
3. **macos-sys: input_source.rs**
   - `current_input_source_id() -> String`: `TISCopyCurrentKeyboardInputSource`
   - `is_cjk_input_source() -> bool`: 检查 ID 包含 CJK 关键字
   - `select_ascii_input_source()`: 查找 ABC/US 并 `TISSelectInputSource`
   - `select_input_source(id)`: 按 ID 切换
4. **macos-sys: key_inject.rs**
   - `simulate_cmd_v()`: `CGEventCreateKeyboardEvent` + `CGEventSetFlags` + `CGEventPost`
5. **app/src/text_inject.rs**
   - `inject_text(text)`: 完整编排 7 步（保存剪贴板→切输入法→写剪贴板→Cmd+V→延迟→恢复）
   - 延迟通过 `cx.start_timeout(0.05)` 实现
6. **主流程串联**
   - 松开 Option → WAV → 显示 "Transcribing..." → HTTP → 收到文本 → 显示文本 → inject → 退场
   - 错误处理: API 不可用 → "Service unavailable" → 3s 后退场
7. **验证** — 录音→转录→文字出现在 TextEdit 中

### Review Checkpoint

- [ ] 对着麦克风说中文，文字出现在 TextEdit
- [ ] CJK 输入法自动切换再恢复
- [ ] 原始剪贴板内容恢复
- [ ] ominix-api 未启动时显示错误不崩溃

---

## Phase 4: LLM Refine + 设置窗口

**Spec:** `specs/phase4-llm-settings.spec.md` | **Estimate:** 1.5d | **Scenarios:** 12

### Steps

1. **app/src/config.rs**
   - `AppConfig` struct (serde Serialize/Deserialize)
   - `load_config() -> AppConfig`: 从文件加载，损坏/缺失时返回默认值
   - `save_config(config)`: 写入 JSON
   - 默认值: language "zh", hotkey OptionLeft/Hold, LLM disabled
2. **app/src/llm_refine.rs**
   - `send_refine_request(cx, text, config)`: POST /v1/chat/completions
   - system prompt 硬编码（保守纠错）
   - `handle_refine_response(response) -> Result<String>`
   - 失败时降级: 返回原始转录文本
3. **设置窗口 UI** — 第三个 Window (script_mod!)
   - API Base URL, Language DropDown, Hotkey DropDown
   - LLM Toggle + API URL/Key/Model inputs
   - Test Connection + Save buttons
   - 状态 Label
4. **菜单栏完整菜单** — 更新 macos-sys `update_menu`
   - Language 子菜单 (zh/en/zh-TW/ja/ko)
   - LLM Refinement 子菜单 (Enable/Disable + Settings)
   - 菜单事件 → 配置更新 + UI 刷新
5. **主流程增加 LLM refine 步骤**
   - 转录完成 → if LLM enabled → "Refining..." → HTTP → 最终文本 → inject
6. **验证** — 设置窗口打开/保存，LLM 纠错 "配森"→"Python"

### Review Checkpoint

- [ ] Settings 窗口打开，修改保存后重启仍生效
- [ ] LLM refine 开关工作正常
- [ ] 菜单栏语言切换生效
- [ ] Test Connection 成功/失败显示正确状态

---

## Phase 5: 动画打磨 + 打包

**Spec:** `specs/phase5-polish.spec.md` | **Estimate:** 1d | **Scenarios:** 8

### Steps

1. **动画细化**
   - 入场: 确认 OutElastic 0.35s 感觉自然
   - 退场: 确认 InCubic 0.22s 不突兀
   - 文字宽度过渡: NextFrame 插值 capsule_window resize，0.25s 平滑
2. **菜单栏图标状态**
   - 录音中: `set_status_bar_icon(&handle, &red_icon_data)`
   - 空闲: 恢复默认图标
3. **悬浮窗定位**
   - 获取主屏幕尺寸 → 计算水平居中 + 底部 80px
   - `capsule_window.reposition(cx, position)`
4. **边界情况**
   - 连续多次录音: 确认无 channel 堆积、buffer 泄漏
   - 快速 toggle: 录音中松开→新录音，不崩溃
5. **.app bundle 打包**
   - Makefile: `build`, `run`, `bundle`, `clean` targets
   - `bundle`: 创建 `VoiceInput.app/Contents/{MacOS,Resources,Info.plist}`
   - Info.plist: `LSUIElement = true`, bundle identifier
   - 拷贝 release binary 到 MacOS/
6. **验证** — 双击 .app 启动，全流程 10 次无异常

### Review Checkpoint

- [ ] 动画流畅，无闪烁
- [ ] .app bundle 双击可用
- [ ] 10 次连续录音无泄漏
