spec: task
name: "Phase 1: 项目骨架 + 悬浮窗"
inherits: project
tags: [skeleton, ui, capsule]
estimate: 1d
---

## 意图

搭建 Cargo workspace 骨架（macos-sys + app 两个 crate），实现 Makepad 应用启动、隐藏 Dock 图标、静态胶囊悬浮窗 UI（含波形 shader 占位），以及 macos-sys 最小实现（NSStatusBar 图标 + Quit 菜单项）。这是所有后续 Phase 的基础。

## 已定决策

- Workspace 根 `Cargo.toml` 定义 `members = ["macos-sys", "app"]`
- `macos-sys` 依赖: `objc2`, `objc2-foundation`, `objc2-app-kit`, `core-foundation`, `block2`, `crossbeam-channel`, `thiserror`
- `app` 依赖: `makepad-widgets` (path), `macos-sys` (path), `serde`, `serde_json`, `anyhow`
- `makepad-widgets` path: `../../FW/robius/makepad/widgets`
- 悬浮窗使用第二个 `Window`，`show_caption_bar: false`
- Startup 时: `cx.show_in_dock(false)` + configure floating panel + 隐藏主窗口
- 波形 shader: 5 根竖条 SDF2D rounded rect，instance 变量 `bar0`-`bar4`，初始值 0.15
- 胶囊外观: 高度 56px，border_radius 28.0，半透明背景 `#x1a1a2e80`

## 边界

### 允许修改
- Cargo.toml
- macos-sys/**
- app/**

### 禁止做
- 不要实现音频录制（Phase 2）
- 不要实现 HTTP 调用（Phase 3）
- 不要实现设置窗口（Phase 4）
- 热键监听只做 stub（打印日志），不要实现完整 CGEvent tap

## 完成条件

场景: Workspace 构建成功
  测试: test_workspace_builds
  假设 Cargo workspace 已创建，包含 macos-sys 和 app 两个 crate
  当 执行 `cargo build --workspace`
  那么 编译成功，无 error
  并且 clippy 无 warning

场景: 应用启动后 Dock 无图标
  测试: test_no_dock_icon
  假设 应用已编译
  当 执行 `cargo run -p makepad-voice-input`
  那么 应用进程启动
  并且 macOS Dock 中不显示应用图标

场景: 菜单栏显示图标
  测试: test_status_bar_icon
  假设 应用已启动
  当 查看 macOS 菜单栏
  那么 菜单栏显示麦克风图标
  并且 点击图标弹出菜单包含 "Quit" 项
  并且 点击 "Quit" 后应用退出

场景: 悬浮窗 UI 显示正确
  测试: test_capsule_window_visible
  假设 应用已启动
  当 悬浮窗设为可见（临时测试入口）
  那么 屏幕上出现无边框胶囊形浮窗
  并且 浮窗高度为 56px，圆角 28
  并且 浮窗包含 5 根竖条波形占位（静态）和 "Listening..." 文字
  并且 浮窗不抢占焦点（non-activating）

场景: 悬浮窗无标题栏无红绿灯
  测试: test_capsule_no_chrome
  假设 悬浮窗已显示
  当 观察悬浮窗
  那么 无标题栏
  并且 无关闭/最小化/最大化按钮

场景: Quit 菜单项异常 — 点击其他菜单项不崩溃
  测试: test_menu_unknown_action
  假设 菜单栏已创建
  当 菜单包含一个无效 action_id 的菜单项被点击
  那么 应用不崩溃
  并且 忽略未知 action
