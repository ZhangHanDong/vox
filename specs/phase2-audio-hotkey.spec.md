spec: task
name: "Phase 2: 音频录制 + 全局热键"
inherits: project
tags: [audio, hotkey, waveform, cgevent]
depends: [phase1-skeleton]
estimate: 2d
---

## 意图

实现全局 Option 键监听（CGEvent tap）和音频录制。按住 Option 开始录音并显示悬浮窗，RMS 实时驱动波形 shader 动画。松开 Option 停止录音并编码 WAV。这是语音输入流程的前半段。

## 已定决策

- CGEvent tap 监听 `kCGEventFlagsChanged`，检测左 Option flag (`NX_DEVICELALTKEYMASK`)
- CGEvent callback 返回 NULL 抑制原始事件（防止触发系统行为）
- 独立线程运行 `CFRunLoopRun`，通过 `CFRunLoopStop` 停止
- 热键事件通过 crossbeam channel 传给 Makepad 主线程
- Makepad 主线程用 10ms interval timer poll channel
- `cx.audio_input(0, callback)` 捕获音频，回调中只做 RMS 计算 + PCM 累积
- RMS 传递: `Arc<AtomicU64>`（`f32::to_bits` 编码）
- PCM 累积: `Arc<Mutex<Vec<f32>>>`，回调中用 `try_lock`
- NextFrame 循环驱动波形: 读 RMS → 平滑包络 (attack 40%, release 15%) → 更新 bar0-bar4
- 竖条权重: `[0.5, 0.8, 1.0, 0.75, 0.55]`，每根 ±4% 随机抖动
- WAV 编码: 16kHz mono 16-bit PCM（ominix-api 要求）
- 入场动画: OutElastic 0.35s (scale 0→1)
- 退场动画: InCubic 0.22s (scale 1→0)

## 边界

### 允许修改
- macos-sys/src/event_tap.rs
- app/src/main.rs
- app/src/audio.rs（新增）

### 禁止做
- 不要实现 HTTP 转录调用（Phase 3）
- 不要在音频回调中分配内存或调用 `Mutex::lock`
- 不要实现文字注入（Phase 3）

## 完成条件

场景: Option 键按下触发录音
  测试: test_option_key_starts_recording
  假设 应用已启动且全局热键监听已安装
  当 用户按住左 Option 键
  那么 应用收到 `HotkeyEvent::Pressed`
  并且 `cx.audio_input` 开始捕获音频
  并且 悬浮窗显示并播放入场动画

场景: Option 键松开停止录音
  测试: test_option_key_stops_recording
  假设 用户正在按住 Option 键录音
  当 用户松开 Option 键
  那么 应用收到 `HotkeyEvent::Released`
  并且 音频捕获停止
  并且 PCM 数据被编码为 16kHz mono WAV
  并且 悬浮窗播放退场动画

场景: 波形动画响应音量
  测试: test_waveform_responds_to_rms
  假设 用户正在录音
  当 用户说话（RMS > 0.01）
  那么 波形竖条高度明显增大
  并且 中间竖条（bar2, 权重 1.0）最高
  并且 两侧竖条（bar0, bar4）较低

场景: 静音时波形最小
  测试: test_waveform_idle_state
  假设 用户正在录音但保持静音
  当 RMS 接近 0
  那么 5 根竖条高度保持最小值（约 4px）
  并且 竖条仍有微小抖动（±4% jitter）

场景: 音频回调不阻塞
  测试: test_audio_callback_nonblocking
  假设 音频回调已安装
  当 `Mutex` 被主线程持有
  那么 音频回调中的 `try_lock` 返回 `Err`
  并且 回调正常返回不阻塞
  并且 该帧音频数据被丢弃

场景: CGEvent tap 需要辅助功能权限
  测试: test_cgevent_tap_permission_denied
  假设 应用未获得辅助功能权限
  当 尝试创建 CGEvent tap
  那么 `start_hotkey_monitor` 返回 `Err`
  并且 应用不崩溃
  并且 菜单栏图标显示权限提示

场景: 短按 Option（<100ms）不触发录音
  测试: test_short_press_ignored
  假设 应用已启动
  当 用户快速按下并松开 Option 键（间隔 < 100ms）
  那么 不启动录音
  并且 悬浮窗不显示
