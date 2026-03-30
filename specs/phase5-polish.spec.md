spec: task
name: "Phase 5: 动画打磨 + 打包"
inherits: project
tags: [animation, polish, bundle]
depends: [phase4-llm-settings]
estimate: 1d
---

## 意图

打磨用户体验细节：入场/退场动画流畅度、文字宽度弹性过渡、录音时菜单栏图标变红、错误状态处理。最终将应用打包为可分发的 .app bundle。

## 已定决策

- 入场动画: Animator OutElastic 0.35s，scale 0→1 + opacity 0→1
- 退场动画: Animator InCubic 0.22s，scale 1→0 + opacity 1→0
- 文字宽度过渡: `capsule_window.resize(cx, new_size)` 配合 0.25s 平滑（通过 NextFrame 插值）
- 录音时菜单栏图标: 调用 `set_status_bar_icon(&handle, &red_icon_data)`
- .app bundle: 手动创建 `VoiceInput.app/Contents/` 目录结构 + Info.plist + 二进制拷贝
- Info.plist 包含 `LSUIElement = true`

## 边界

### 允许修改
- app/src/main.rs
- macos-sys/src/status_bar.rs
- scripts/**（新增打包脚本）
- Makefile（新增）

### 禁止做
- 不要修改核心转录/注入逻辑
- 不要添加新依赖
- 不要修改 macos-sys 公开 API 签名

## 完成条件

场景: 入场动画流畅
  测试: test_entrance_animation
  假设 悬浮窗处于隐藏状态
  当 用户按住 Option 键
  那么 悬浮窗从 scale 0 弹出到 scale 1
  并且 使用 OutElastic 缓动
  并且 动画时长约 0.35 秒

场景: 退场动画流畅
  测试: test_exit_animation
  假设 悬浮窗正在显示
  当 文字注入完成
  那么 悬浮窗从 scale 1 缩小到 scale 0
  并且 opacity 从 1 渐变到 0
  并且 动画时长约 0.22 秒
  并且 动画结束后窗口隐藏

场景: 文字宽度弹性扩展
  测试: test_capsule_width_transition
  假设 悬浮窗显示 "Listening..."（短文本）
  当 转录返回较长文本 "今天的天气非常好，适合出去散步"
  那么 胶囊宽度在约 0.25 秒内平滑扩展到适合文本的宽度
  并且 扩展过程中不闪烁

场景: 录音时菜单栏图标变红
  测试: test_status_bar_icon_recording
  假设 应用处于空闲状态，菜单栏显示默认麦克风图标
  当 用户按住 Option 键开始录音
  那么 菜单栏图标变为红色版本
  并且 录音结束后图标恢复为默认颜色

场景: 连续多次录音不泄漏
  测试: test_no_resource_leak
  假设 应用已启动
  当 用户连续执行 10 次录音-转录-注入流程
  那么 每次悬浮窗正常显示和退场
  并且 无内存泄漏（音频 buffer 被回收）
  并且 无 channel 阻塞

场景: .app bundle 可双击启动
  测试: test_app_bundle_launches
  假设 执行 `make bundle` 生成 VoiceInput.app
  当 双击 VoiceInput.app
  那么 应用启动
  并且 Dock 无图标
  并且 菜单栏显示图标

场景: 悬浮窗定位在屏幕底部居中
  测试: test_capsule_position
  假设 应用已启动
  当 悬浮窗显示
  那么 悬浮窗水平居中于主屏幕
  并且 悬浮窗底部距屏幕底边约 80px

场景: 录音中快速松开再按住
  测试: test_rapid_toggle
  假设 用户正在录音
  当 用户松开 Option 后 200ms 内再次按住
  那么 第一次录音正常结束并触发转录
  并且 第二次按住开始新的录音
  并且 应用不崩溃
