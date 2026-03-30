spec: task
name: "Phase 3: 转录 + 文字注入"
inherits: project
tags: [transcribe, inject, clipboard, http]
depends: [phase2-audio-hotkey]
estimate: 2d
---

## 意图

实现录音结束后的完整流程：将 WAV 音频通过 HTTP 发送到 ominix-api 进行转录，收到文本后通过剪贴板 + 模拟 Cmd+V 注入到当前聚焦的输入框。注入前处理 CJK 输入法切换，注入后恢复原始剪贴板和输入法状态。

## 已定决策

- HTTP 请求使用 Makepad `HttpRequest`，`POST /v1/audio/transcriptions`
- Content-Type: `multipart/form-data`，字段: `file` (WAV bytes), `language` (配置语言)
- 响应格式: JSON `{"text": "转录文本"}`
- 悬浮窗状态流转: "Listening..." → "Transcribing..." → 显示转录文本 → 退场
- 文字注入编排顺序:
  1. `read_clipboard()` 保存原内容
  2. `is_cjk_input_source()` 检测当前输入法
  3. 若 CJK → `select_ascii_input_source()` 切换
  4. `write_clipboard(&text)` 写入转录文本
  5. `simulate_cmd_v()` 模拟粘贴
  6. 50ms 延迟 (cx.start_timeout)
  7. `select_input_source(&original_id)` 恢复输入法
  8. `write_clipboard(&original)` 恢复剪贴板
- 输入法 CJK 判断: ID 包含 "SCIM", "inputmethod.Chinese", "inputmethod.Japanese", "inputmethod.Korean", "TCIM"

## 边界

### 允许修改
- macos-sys/src/clipboard.rs（新增）
- macos-sys/src/input_source.rs（新增）
- macos-sys/src/key_inject.rs（新增）
- app/src/transcribe.rs（新增）
- app/src/text_inject.rs（新增）
- app/src/main.rs

### 禁止做
- 不要实现 LLM refine（Phase 4）
- 不要修改 Phase 2 的音频录制逻辑
- 不要使用外部 HTTP 库（使用 Makepad 内置 HttpRequest）

## 完成条件

场景: 转录请求发送成功
  测试: test_transcribe_request_sent
  假设 录音已完成，WAV 数据已编码
  当 松开 Option 键
  那么 悬浮窗显示 "Transcribing..."
  并且 HTTP POST 发送到 `{base_url}/v1/audio/transcriptions`
  并且 请求包含 WAV 文件和语言参数

场景: 转录结果显示在悬浮窗
  测试: test_transcribe_result_displayed
  假设 转录请求已发送
  当 ominix-api 返回 `{"text": "你好世界"}`
  那么 悬浮窗显示 "你好世界"
  并且 悬浮窗宽度随文字长度弹性扩展

场景: 文字注入到当前输入框
  测试: test_text_injected_via_clipboard
  假设 转录文本为 "你好世界"
  并且 当前聚焦窗口有一个文本输入框
  当 注入流程执行
  那么 文本 "你好世界" 出现在输入框中
  并且 注入完成后悬浮窗播放退场动画

场景: CJK 输入法被临时切换
  测试: test_cjk_input_source_switched
  假设 当前输入法为搜狗拼音 (CJK)
  当 注入流程执行
  那么 注入前输入法切换到 ABC/US
  并且 模拟 Cmd+V 粘贴
  并且 注入后输入法恢复为搜狗拼音

场景: 非 CJK 输入法不切换
  测试: test_non_cjk_no_switch
  假设 当前输入法为 ABC (非 CJK)
  当 注入流程执行
  那么 不执行输入法切换
  并且 直接模拟 Cmd+V

场景: 原始剪贴板内容被恢复
  测试: test_clipboard_restored
  假设 剪贴板原始内容为 "原始文本"
  当 注入流程完成
  那么 剪贴板内容恢复为 "原始文本"

场景: 空转录结果不注入
  测试: test_empty_transcription_skipped
  假设 录音时用户未说话
  当 ominix-api 返回 `{"text": ""}`
  那么 不执行文字注入
  并且 悬浮窗直接播放退场动画

场景: ominix-api 连接失败
  测试: test_transcribe_api_unreachable
  假设 ominix-api 服务未运行
  当 发送转录请求
  那么 悬浮窗显示 "Service unavailable"
  并且 3 秒后自动退场
  并且 不执行文字注入
  并且 应用不崩溃

场景: ominix-api 返回错误状态码
  测试: test_transcribe_api_error
  假设 ominix-api 返回 500 错误
  当 处理 HTTP 响应
  那么 悬浮窗显示 "Transcription failed"
  并且 3 秒后自动退场
