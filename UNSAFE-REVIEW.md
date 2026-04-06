# Unsafe Rust

## 概念锚点

### 正确性论证
参考 **Ralf Jung**（Miri 作者、Stacked/Tree Borrows 提出者）的方法论：

- 每个 `unsafe` block 必须有 `// SAFETY:` 注释，说明：
  1. 这段代码依赖的 safety invariant 是什么
  2. 为什么这些 invariant 在此处成立
  3. 调用者需要满足的 precondition（如果是 unsafe fn）
- 用 Miri 验证：`cargo +nightly miri test` 能捕获大量未定义行为（越界访问、悬垂指针、数据竞争、违反别名规则）
- Miri 不能证明正确，只能发现错误——通过 Miri 不等于没有 UB

### 封装策略
参考 **BurntSushi** 在 `memchr`、`regex` 中的实践：

- unsafe 的存在对公共 API 的使用者必须不可见
- 内部模块用 unsafe 实现性能关键路径（SIMD、手工内存管理），外部接口是完全 safe 的
- 在 safe wrapper 中验证所有 precondition，使 unsafe block 内部的 invariant 成为私有实现细节

### 使用决策
参考 **dtolnay** 的节制：

- 先证明 safe 方案确实不可行（性能不达标、功能不可实现），再考虑 unsafe
- 如果 unsafe 的唯一理由是"少写几行代码"或"稍微快一点"，不用
- 如果 safe 方案存在但需要运行时检查（如 bounds check），在性能非瓶颈处优先用 safe 方案

### 并发相关 unsafe
参考 **Mara Bos**（《Rust Atomics and Locks》作者）：

- 原子操作的 ordering 选择不是玄学——`Relaxed` 不保证任何跨线程可见性顺序，`Acquire/Release` 配对建立 happens-before 关系，`SeqCst` 保证全序
- 默认选 `SeqCst` 除非你能证明更弱的 ordering 是正确的
- 自己实现 lock-free 数据结构之前先看 `crossbeam` 有没有现成的

## 检查清单

unsafe 代码提交前的质检（不是风格检查，是正确性检查）：

```bash
# 1. Miri 检测未定义行为
cargo +nightly miri test

# 2. 地址检查器（ASan）
RUSTFLAGS="-Z sanitizer=address" cargo +nightly test

# 3. 线程检查器（TSan）
RUSTFLAGS="-Z sanitizer=thread" cargo +nightly test

# 4. clippy 的 unsafe 相关 lint
cargo clippy -- -W clippy::undocumented_unsafe_blocks
```

## FFI 专项

跨语言边界（C/C++ 互操作）的 unsafe 有额外的关注点：

- 用 `#[repr(C)]` 确保内存布局兼容
- 空指针检查在 Rust 侧做——不要信任 C 端传入的指针
- 字符串转换用 `CStr`/`CString`，不要手工处理 null terminator
- 考虑用 `cbindgen`（Rust → C 头文件）或 `bindgen`（C 头文件 → Rust FFI）自动生成绑定
- 回调函数传递时注意 `extern "C"` ABI 和 panic 边界——Rust panic 不能跨 FFI 边界传播
