# Unsafe Rust 审查报告（Codex）

> 日期：2026-04-06
> 审查人：Codex
> 分支：`feat/meeting-minutes`
> 提交：`0007150`
> 审查依据：[`UNSAFE-REVIEW.md`](../UNSAFE-REVIEW.md)

## 摘要

本次审查按 [`UNSAFE-REVIEW.md`](../UNSAFE-REVIEW.md) 的四个核心视角进行：

1. 正确性论证：每个 `unsafe` / `extern "C"` / `unsafe impl` 依赖什么不变量
2. 封装策略：safe wrapper 是否把前置条件完整封住
3. FFI 专项：指针、所有权、panic 边界是否成立
4. 并发专项：`Send` / `Sync`、跨线程可见性、线程亲和性是否有充分论证

结论：

- 发现 1 个高严重性问题：`extern "C"` 回调中直接调用 Rust 闭包，safe API 未封住 panic 穿越 FFI 边界的风险
- 发现 1 个中严重性问题：`StatusBarHandle` 的 `unsafe impl Send` 缺乏线程亲和性论证
- 发现 1 个低严重性流程问题：项目未满足 [`UNSAFE-REVIEW.md`](../UNSAFE-REVIEW.md) 要求的 `// SAFETY:` 注释规范

除上述问题外，我没有在 `clipboard.rs`、`input_source.rs`、`key_inject.rs` 以及 `app/src/main.rs` 的那处 `NSScreen` FFI 中看到同等级别、足以直接定性的未定义行为证据。

## 审查范围

通过全文检索，当前 unsafe 面主要分布在以下文件：

- `macos-sys/src/event_tap.rs`
- `macos-sys/src/status_bar.rs`
- `macos-sys/src/clipboard.rs`
- `macos-sys/src/input_source.rs`
- `macos-sys/src/key_inject.rs`
- `app/src/main.rs`

检索命令：

```bash
rg -n --hidden --glob '!target' '\bunsafe\b|extern \"C\"|unsafe impl|unsafe fn' macos-sys/src app/src
```

## 审查方法

### 1. 按不变量审，不按“有没有 unsafe”审

对每一处 unsafe，我都问同一个问题：

- 这段代码依赖什么 safety invariant？
- 这个 invariant 在当前实现里是谁保证的？
- 如果 API 是 safe 的，调用者是否可能在不知情的情况下破坏该 invariant？

这一步对应 [`UNSAFE-REVIEW.md`](../UNSAFE-REVIEW.md) 中“正确性论证”和“封装策略”的要求。

### 2. 单独追 FFI 边界

对所有 `extern "C"` 回调，我重点检查：

- Rust panic 是否可能穿过 FFI 边界
- 传入指针是否在解引用前做了足够验证
- 所有权转移是否可闭合

这一步对应 [`UNSAFE-REVIEW.md`](../UNSAFE-REVIEW.md) 中“FFI 专项”的要求。

### 3. 单独追跨线程边界

对所有 `unsafe impl Send`、原子变量和后台线程句柄，我重点检查：

- 类型是否真的可以跨线程发送
- 线程亲和性是否被类型系统或运行时显式约束

这一步对应 [`UNSAFE-REVIEW.md`](../UNSAFE-REVIEW.md) 中“并发相关 unsafe”的要求。

## 发现

### 高严重性：`start_hotkey_monitor` 的 safe 封装没有封住 panic 穿越 FFI 边界

证据链：

- [`macos-sys/src/event_tap.rs:165`](../macos-sys/src/event_tap.rs#L165) 暴露的是 safe API：`pub fn start_hotkey_monitor(...)`
- [`macos-sys/src/event_tap.rs:112`](../macos-sys/src/event_tap.rs#L112) 定义了 `extern "C" fn event_tap_callback`
- [`macos-sys/src/event_tap.rs:135`](../macos-sys/src/event_tap.rs#L135) 和 [`macos-sys/src/event_tap.rs:150`](../macos-sys/src/event_tap.rs#L150) 在 FFI 回调里直接调用 `(ctx.callback)(...)`
- 整个回调体没有 `std::panic::catch_unwind`
- 当前调用点在 [`app/src/main.rs:331`](../app/src/main.rs#L331)，目前只是 `try_send`，但这只能说明“当前调用方大概率不 panic”，不能证明这个 safe API 本身是 sound 的

为什么这是问题：

- [`UNSAFE-REVIEW.md`](../UNSAFE-REVIEW.md) 明确要求 safe wrapper 负责把 unsafe 前置条件封装掉，而不是把前置条件泄露给调用者
- 对一个 safe API 来说，“传入的闭包不能 panic”不能作为默认前置条件
- 一旦 future 调用方在闭包中 panic，unwind 将穿过 `extern "C"` 边界；这不是当前 safe API 可以合法暴露给调用者的行为边界

建议修复：

- 在 `event_tap_callback` 内部用 `catch_unwind(AssertUnwindSafe(...))` 包裹对 `ctx.callback` 的调用
- panic 后记录日志并返回一个保守结果，而不是让 unwind 继续穿过 CoreGraphics 回调边界

### 中严重性：`StatusBarHandle` 的 `unsafe impl Send` 缺乏成立依据

证据链：

- [`macos-sys/src/status_bar.rs:97`](../macos-sys/src/status_bar.rs#L97) 中 `StatusBarHandle` 持有 `NSStatusItem` 的原始指针
- [`macos-sys/src/status_bar.rs:101`](../macos-sys/src/status_bar.rs#L101) 对该类型声明了 `unsafe impl Send`
- 该句柄后续会被安全方法直接操作：
  - [`macos-sys/src/status_bar.rs:181`](../macos-sys/src/status_bar.rs#L181) `update_menu`
  - [`macos-sys/src/status_bar.rs:188`](../macos-sys/src/status_bar.rs#L188) `set_status_bar_icon`
  - [`macos-sys/src/status_bar.rs:198`](../macos-sys/src/status_bar.rs#L198) `Drop`
- `Drop` 中会调用 [`macos-sys/src/status_bar.rs:200`](../macos-sys/src/status_bar.rs#L200) 到 [`macos-sys/src/status_bar.rs:203`](../macos-sys/src/status_bar.rs#L203) 的 AppKit 消息发送

为什么这是问题：

- `unsafe impl Send` 的含义不是“当前调用路径碰巧只在主线程使用”，而是“任意 safe Rust 代码都可以把这个值发送到别的线程，依然不会破坏不变量”
- 当前实现没有建立任何“只能在主线程 drop / 调用”的类型约束，也没有运行时线程检查
- 因而这个 `Send` 断言并没有被代码中的不变量支撑

为什么我把它定为中严重性而不是高严重性：

- 目前应用中的实际存活路径是把它放在 `Inner` 里，现有调用点看起来仍在 UI 主线程
- 也就是说，当前更像是“类型系统错误地放宽了边界”，而不是已经从现有调用路径中证明了立即触发 UB
- 但这个边界一旦被后续代码误用，风险会直接落到 safe Rust 层面

建议修复：

- 首选：移除 `unsafe impl Send`
- 如果业务上确实需要跨线程持有，则必须显式设计线程跳转或主线程代理，而不是让 AppKit 对象本身跨线程移动

### 低严重性：unsafe 审查约定没有落地，导致安全不变量不可审计

证据链：

- [`UNSAFE-REVIEW.md`](../UNSAFE-REVIEW.md) 要求每个 `unsafe` block 都带 `// SAFETY:` 注释
- 运行以下命令后，clippy 明确报出多处未注释的 unsafe：

```bash
cargo clippy -p macos-sys -- -W clippy::undocumented_unsafe_blocks
cargo clippy -p vox -- -W clippy::undocumented_unsafe_blocks
```

- 典型位置包括：
  - [`macos-sys/src/event_tap.rs:118`](../macos-sys/src/event_tap.rs#L118)
  - [`macos-sys/src/status_bar.rs:101`](../macos-sys/src/status_bar.rs#L101)
  - [`app/src/main.rs:872`](../app/src/main.rs#L872)

为什么这是问题：

- 这不直接等于 UB
- 但它会让审查者无法快速确认每一处 unsafe 的成立条件，也无法确认调用者需要遵守什么边界
- 对 FFI 和线程亲和性代码而言，这种“没有显式安全论证”的状态会显著提高后续回归风险

建议修复：

- 为每个 `unsafe` block、`unsafe fn`、`unsafe impl` 补齐 `// SAFETY:` 注释
- 注释内容应明确写出：
  - 依赖的不变量
  - 当前实现为何满足这些不变量
  - 如果是 `unsafe fn`，调用方必须满足什么前置条件

## 本次未升级为问题的点

以下代码我检查过，但当前证据不足以把它们升级为本报告的正式 finding：

### `clipboard.rs` / `input_source.rs` 的字符串桥接

- [`macos-sys/src/clipboard.rs:22`](../macos-sys/src/clipboard.rs#L22) 到 [`macos-sys/src/clipboard.rs:31`](../macos-sys/src/clipboard.rs#L31)
- [`macos-sys/src/input_source.rs:33`](../macos-sys/src/input_source.rs#L33) 到 [`macos-sys/src/input_source.rs:43`](../macos-sys/src/input_source.rs#L43)

理由：

- 两处都先做了空指针检查
- `UTF8String` 返回后立刻用 `CStr::from_ptr` 读取，并马上复制到 Rust `String`
- 从当前代码看，没有把底层缓冲区借用跨出函数作用域

### `app/src/main.rs` 的 `NSScreen` 调用

- [`app/src/main.rs:872`](../app/src/main.rs#L872) 到 [`app/src/main.rs:878`](../app/src/main.rs#L878)

理由：

- 对 `mainScreen` 做了空值判断
- 局部 `NSRect` 使用了 `#[repr(C)]`
- 这是一次同步读取，不涉及所有权转移和跨线程共享

### `user_info` 空指针检查

我没有把 [`macos-sys/src/event_tap.rs:119`](../macos-sys/src/event_tap.rs#L119) 缺少显式 `null` 检查单独列为正式问题。

理由：

- `user_info` 是由当前代码在 [`macos-sys/src/event_tap.rs:200`](../macos-sys/src/event_tap.rs#L200) 主动传入 `CGEventTapCreate`
- 当前成功路径下并没有把 `null` 作为实参传入
- 从本仓库代码本身可以论证“注册时传入的是有效指针”

这仍然可以作为健壮性增强项，但证据强度不足以作为本次主结论。

## 验证记录

本次实际执行过的命令：

```bash
rg -n --hidden --glob '!target' '\bunsafe\b|extern \"C\"|unsafe impl|unsafe fn' macos-sys/src app/src
cargo test --workspace --no-run
cargo clippy -p macos-sys -- -W clippy::undocumented_unsafe_blocks
cargo clippy -p vox -- -W clippy::undocumented_unsafe_blocks
```

结果：

- `cargo test --workspace --no-run` 通过，说明当前工作树在测试构建层面可编译
- 两个 clippy 命令均成功执行，但报告了多处 `undocumented_unsafe_blocks`

## 结论

按 [`UNSAFE-REVIEW.md`](../UNSAFE-REVIEW.md) 的标准，这个仓库当前的 unsafe 代码总体上还算收敛，主要集中在 `macos-sys` 这个 FFI crate，方向是对的；但仍有两处边界没有被完整证明：

1. safe API 没有封住 FFI panic 边界
2. `Send` 断言没有被线程亲和性不变量支撑

这两处都不应该靠“当前调用路径恰好没出事”来维持。建议先修这两项，再补齐全量 `// SAFETY:` 注释，把安全论证真正写进代码。
