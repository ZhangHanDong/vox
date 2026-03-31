spec: task
name: "Meeting Minutes Feature"
tags: [meeting, feature, v0.2]
estimate: 2d
---

## 意图

为 Vox 新增"会议纪要"模式。用户通过菜单启动会议录音，系统持续录音并每 30 秒自动分段转录，实时累积文本。会议结束后，将全部转录文本发送给 LLM 生成结构化纪要（摘要、要点、决策、Action Items），最终保存为带时间戳的 Markdown 文件。

此功能独立于现有的"按住 Option 说话"语音输入功能，两者通过菜单切换，不互相干扰。

## 已定决策

- 会议模式通过菜单栏 Start/Stop Meeting 控制，不使用 Option 热键
- 录音期间 Option 热键被禁用（防止误触）
- 音频每 30 秒自动分段（可配置），每段独立发送 ASR 转录
- 转录请求串行发送（同时最多一个 in-flight），其余排队
- 会议结束后全文发给 LLM 生成结构化纪要（可配置关闭）
- 每次会议保存为独立目录: `~/Documents/Vox/meeting_YYYY-MM-DD_HHMMSS/`
- 目录内包含两个文件:
  - `transcript.md` — 完整原始转录文本（每个 chunk 带时间戳，逐字记录）
  - `summary.md` — LLM 生成的结构化纪要（摘要/要点/决策/Action Items）
- 即使 LLM 纪要生成失败，transcript.md 仍然保存
- 胶囊窗口在会议期间显示录音时长和分段计数
- 菜单栏图标在会议期间变为 "📝"
- 新增 `app/src/meeting.rs` 模块，独立封装会议逻辑
- 配置新增 `MeetingConfig` 段，独立于现有 `llm_refine` 配置
- LLM 纪要请求 max_tokens 提升到 4096（现有纠错用 2048）

## 约束

- 不修改现有语音输入功能的行为（STATE_IDLE/RECORDING/TRANSCRIBING/REFINING 不变）
- 会议状态使用独立编号段（10+），不干扰现有 0-3 状态
- `drain_chunk()` 在主线程调用 `lock()`（非 RT 线程），可接受短暂竞争
- 音频回调线程的 `try_lock` 行为不变
- LLM 上下文窗口限制：约 2 小时会议（32k tokens）。超长会议需截断（记录为已知限制）
- Splash DSL 不添加新 instance 变量，会议状态仅通过 `set_text()` 反映

## 边界

### 允许修改
- app/src/meeting.rs（新增）
- app/src/main.rs（新增会议状态、菜单、流程）
- app/src/audio.rs（新增 drain_chunk）
- app/src/config.rs（新增 MeetingConfig）
- app/src/transcribe.rs（新增 MEETING_CHUNK_REQUEST_ID）
- app/src/llm_refine.rs（新增 LLM_SUMMARY_REQUEST_ID）
- specs/meeting-minutes.spec.md

### 禁止做
- 不修改 text_inject.rs（会议纪要不注入剪贴板）
- 不修改 macos-sys crate 的公共 API
- 不修改现有语音输入的状态流转
- 不在 Splash DSL 中添加 instance 变量

## 排除范围

- 说话人识别（Speaker diarization）
- 流式转录（实时字幕效果）
- 会议纪要编辑 UI
- 云端同步/分享会议纪要
- 多语言混合会议（整个会议使用同一语言设置）

## 完成条件

场景: 菜单中出现 Meeting Mode 子菜单
  测试: test_meeting_menu
  假设 应用已启动
  当 点击 MIC 菜单
  那么 菜单包含 "Meeting Mode" 子菜单
  并且 子菜单包含 "Start Meeting" 项

场景: 启动会议录音
  测试: test_start_meeting
  假设 应用处于空闲状态
  当 点击 MIC → Meeting Mode → Start Meeting
  那么 状态变为 STATE_MEETING_RECORDING
  并且 音频捕获开始
  并且 胶囊窗口显示 "📝 Meeting 00:00 | 0 chunks"
  并且 菜单栏图标变为 "📝"
  并且 30 秒定时器启动

场景: 自动分段转录
  测试: test_auto_chunk
  假设 会议正在录音
  当 30 秒定时器触发
  那么 PCM 缓冲区被 drain（不停止录音）
  并且 WAV 编码后通过 HTTP POST 发送到 ASR
  并且 chunk 计数器加 1
  并且 胶囊显示更新为 "📝 Meeting 00:30 | 1 chunks"

场景: 转录结果累积
  测试: test_chunk_accumulation
  假设 第 3 个 chunk 的转录请求已发送
  当 ASR 返回 "这是第三段内容"
  那么 MeetingSession.chunks 包含 3 条记录
  并且 full_transcript 包含所有 3 段文本
  并且 每段记录有对应的时间戳偏移

场景: 转录请求串行发送
  测试: test_sequential_requests
  假设 第 1 个 chunk 的 HTTP 请求尚未返回
  当 30 秒定时器再次触发（第 2 个 chunk）
  那么 第 2 个 chunk 的 WAV 数据被加入队列
  并且 不发送新的 HTTP 请求
  并且 第 1 个请求返回后，自动发送队列中的下一个

场景: 会议期间 Option 热键被禁用
  测试: test_hotkey_disabled_during_meeting
  假设 会议正在录音
  当 用户按住并松开 Option 键
  那么 不触发普通语音输入
  并且 会议录音不受影响

场景: 停止会议
  测试: test_stop_meeting
  假设 会议正在录音
  当 点击 MIC → Meeting Mode → Stop Meeting
  那么 音频捕获停止
  并且 剩余 PCM 数据作为最后一个 chunk 发送
  并且 状态变为 STATE_MEETING_FINALIZING
  并且 胶囊显示 "📝 Finishing up..."

场景: 等待所有 pending chunk 完成
  测试: test_wait_pending_chunks
  假设 会议已停止，还有 2 个 chunk 未返回
  当 最后一个 chunk 转录返回
  那么 pending_chunks 变为 0
  并且 如果 auto_summary 开启，进入 STATE_MEETING_SUMMARIZING

场景: LLM 生成结构化纪要
  测试: test_llm_summary
  假设 所有 chunk 已转录完成
  并且 auto_summary 配置为 true
  并且 LLM API 已配置
  当 进入 STATE_MEETING_SUMMARIZING
  那么 全部转录文本发送给 LLM（max_tokens: 4096）
  并且 system prompt 要求输出 Summary/Key Points/Decisions/Action Items
  并且 胶囊显示 "📝 Generating summary..."

场景: 保存完整转录文本
  测试: test_save_transcript
  假设 会议结束且所有 chunk 已转录
  当 保存会议数据
  那么 创建目录 ~/Documents/Vox/meeting_YYYY-MM-DD_HHMMSS/
  并且 生成 transcript.md 包含:
    | 内容 | 说明 |
    | 标题 | # Meeting Transcript — YYYY-MM-DD HH:MM |
    | 元信息 | Duration, Language, Chunks |
    | 全文 | 每段带 [MM:SS] 时间戳的原始转录文本 |
  并且 transcript.md 始终保存（不依赖 LLM）

场景: LLM 生成结构化纪要文件
  测试: test_save_summary
  假设 auto_summary 开启且 LLM 返回成功
  当 LLM 纪要生成完成
  那么 在同一目录下生成 summary.md 包含:
    | 章节 | 内容 |
    | Summary | LLM 生成的摘要 |
    | Key Points | LLM 生成的要点 |
    | Decisions | LLM 生成的决策 |
    | Action Items | LLM 生成的待办 |
  并且 胶囊显示 "📝 Saved: /path/to/meeting_.../"
  并且 3 秒后胶囊自动隐藏
  并且 状态恢复为 STATE_IDLE

场景: 无 LLM 时仅保存转录
  测试: test_save_without_summary
  假设 auto_summary 为 false 或 LLM API 未配置
  当 所有 chunk 转录完成
  那么 只生成 transcript.md（无 summary.md）
  并且 transcript.md 包含完整带时间戳的文本

场景: LLM 纪要生成失败时降级
  测试: test_summary_fallback
  假设 LLM API 返回错误或超时
  当 处理 LLM 响应
  那么 transcript.md 已保存（不受影响）
  并且 不生成 summary.md
  并且 胶囊显示 "📝 Summary failed, transcript saved"
  并且 不阻塞流程

场景: chunk 转录失败
  测试: test_chunk_error
  假设 会议正在录音
  当 某个 chunk 的 ASR 请求返回错误
  那么 该 chunk 记录为 "(transcription failed)"
  并且 继续处理后续 chunk
  并且 会议不中断

场景: 会议配置持久化
  测试: test_meeting_config
  假设 配置文件 ~/.config/vox/config.json 存在
  当 读取配置
  那么 包含 meeting.chunk_duration_secs（默认 30）
  并且 包含 meeting.output_dir（默认 ~/Documents/Vox）
  并且 包含 meeting.auto_summary（默认 true）

场景: drain_chunk 不阻塞音频线程
  测试: test_drain_nonblocking
  假设 音频回调正在执行 try_lock
  当 主线程调用 drain_chunk (lock)
  那么 drain_chunk 等待 try_lock 释放后获取锁
  并且 mem::take 交换缓冲区（O(1)）
  并且 音频回调下次 try_lock 成功时写入新的空缓冲区

场景: 停止会议后恢复普通模式
  测试: test_restore_normal_mode
  假设 会议已结束并保存
  当 状态恢复为 STATE_IDLE
  那么 AppMode 恢复为 Normal
  并且 Option 热键重新生效
  并且 菜单栏图标恢复为 "MIC"
  并且 菜单显示 "Start Meeting"（非 "Stop Meeting"）

场景: 输出目录不存在时自动创建
  测试: test_create_output_dir
  假设 ~/Documents/Vox/ 目录不存在
  当 启动会议
  那么 自动创建 ~/Documents/Vox/ 目录
  并且 会议正常开始

场景: transcript.md 文件格式正确
  测试: test_transcript_format
  假设 会议持续 2 分钟，产生 4 个 chunk
  当 会议结束并保存
  那么 transcript.md 包含:
    | 章节 | 内容 |
    | # Meeting Transcript — YYYY-MM-DD HH:MM | 标题 |
    | Duration | 2m 0s |
    | Language | 当前语言设置 |
    | Chunks | 4 |
    | 正文 | 4 段带 [00:00] [00:30] [01:00] [01:30] 时间戳的逐字转录 |

场景: summary.md 文件格式正确
  测试: test_summary_format
  假设 LLM 纪要已生成
  当 保存 summary.md
  那么 文件包含:
    | 章节 | 内容 |
    | # Meeting Summary — YYYY-MM-DD HH:MM | 标题 |
    | ## Summary | 2-3 句概述 |
    | ## Key Points | 要点列表 |
    | ## Decisions | 决策列表（如有） |
    | ## Action Items | 待办列表（如有） |
