spec: task
name: "Phase 4: LLM Refine + 设置窗口"
inherits: project
tags: [llm, settings, config, menu]
depends: [phase3-transcribe-inject]
estimate: 1.5d
---

## 意图

实现 LLM 文本纠错功能和完整的设置界面。转录完成后可选地通过 ominix-api LLM 端点 refine 文本，修复语音识别错误。提供 Makepad 设置窗口供用户配置 API 地址、语言、热键和 LLM 参数。完善菜单栏菜单（语言切换、LLM 开关、Settings 入口）。

## 已定决策

- LLM 调用: `POST {llm_api_base_url}/v1/chat/completions`
- 请求体: `{"model": "...", "messages": [{"role": "system", "content": "..."}, {"role": "user", "content": "转录文本"}]}`
- System prompt: 保守纠错，只修复明显语音识别错误（中文谐音、英文术语被错误转为中文），不改写不润色
- 设置窗口: 第三个 Makepad Window（普通窗口，非 floating）
- 配置文件: `~/.config/makepad-voice-input/config.json`，serde 序列化
- API Key 输入框: `password: true`，支持完全清空
- Test Connection: 发送 `GET {base_url}/v1/models` 验证连通性
- 菜单结构: Language 子菜单 + LLM Refinement 子菜单 (Enable/Disable + Settings) + Quit

## 边界

### 允许修改
- app/src/llm_refine.rs（新增）
- app/src/config.rs（新增）
- app/src/main.rs
- macos-sys/src/status_bar.rs

### 禁止做
- 不要修改音频录制逻辑
- 不要修改转录 HTTP 调用逻辑
- 不要添加第三方 HTTP 库

## 完成条件

场景: LLM refine 纠正谐音错误
  测试: test_llm_refine_fixes_homophone
  假设 LLM refine 已启用且 API 已配置
  并且 转录文本为 "我用配森写代码"
  当 LLM refine 请求发送并返回
  那么 最终注入文本为 "我用 Python 写代码"

场景: LLM refine 不改写正确文本
  测试: test_llm_refine_preserves_correct
  假设 LLM refine 已启用
  并且 转录文本为 "今天天气很好"
  当 LLM refine 请求发送并返回
  那么 最终注入文本为 "今天天气很好"（原样返回）

场景: LLM refine 禁用时跳过
  测试: test_llm_refine_disabled_skip
  假设 LLM refine 已禁用（config.llm_refine.enabled = false）
  当 转录完成
  那么 不发送 LLM 请求
  并且 直接注入转录文本

场景: LLM refine 过程中显示状态
  测试: test_llm_refine_status_display
  假设 LLM refine 已启用
  当 转录完成后发送 LLM 请求
  那么 悬浮窗显示 "Refining..."
  并且 LLM 返回后悬浮窗显示最终文本

场景: LLM API 失败时降级为原始转录
  测试: test_llm_refine_fallback
  假设 LLM refine 已启用但 API 返回错误
  当 处理 LLM 响应
  那么 使用原始转录文本进行注入
  并且 不阻塞注入流程

场景: 设置窗口打开并显示当前配置
  测试: test_settings_window_opens
  假设 配置文件存在且包含有效配置
  当 用户点击菜单栏 "Settings..."
  那么 设置窗口打开
  并且 API Base URL 输入框显示当前配置值
  并且 Language 下拉框选中当前语言
  并且 LLM Toggle 反映当前启用状态

场景: 保存设置到文件
  测试: test_settings_save
  假设 设置窗口已打开
  当 用户修改 API Base URL 为 "http://remote:8080" 并点击 Save
  那么 `~/.config/makepad-voice-input/config.json` 被更新
  并且 文件中 `ominix_api.base_url` 为 "http://remote:8080"
  并且 设置窗口显示 "Saved" 状态

场景: Test Connection 成功
  测试: test_connection_success
  假设 ominix-api 运行在配置的地址
  当 用户点击 "Test Connection"
  那么 设置窗口显示 "Connected" 绿色状态

场景: Test Connection 失败
  测试: test_connection_failure
  假设 配置的 API 地址无服务
  当 用户点击 "Test Connection"
  那么 设置窗口显示 "Connection failed" 红色状态

场景: 语言切换生效
  测试: test_language_switch
  假设 当前语言为 zh
  当 用户在菜单栏选择 "English"
  那么 语言配置更新为 "en"
  并且 后续转录请求的 language 参数为 "en"
  并且 菜单栏语言子菜单中 "English" 显示勾选

场景: 首次启动无配置文件
  测试: test_first_launch_default_config
  假设 `~/.config/makepad-voice-input/config.json` 不存在
  当 应用启动
  那么 使用默认配置（语言 zh, 热键 OptionLeft, LLM 禁用）
  并且 应用正常运行不崩溃

场景: 配置文件损坏
  测试: test_corrupted_config_fallback
  假设 配置文件包含无效 JSON
  当 应用启动
  那么 使用默认配置
  并且 应用正常运行不崩溃
