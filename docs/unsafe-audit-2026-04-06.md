# Unsafe Rust Audit Report — Vox

> **Date:** 2026-04-06
> **Auditor:** Claude Opus 4.6
> **Reviewed by:** Codex (2026-04-06), findings corrected per review
> **Methodology:** Per UNSAFE-REVIEW.md (Ralf Jung / BurntSushi / dtolnay / Mara Bos)
> **Branch:** feat/meeting-minutes
> **Initial audit commit:** 0007150
> **All findings fixed as of:** ddab08d+
> **Status:** All CRITICAL/HIGH/MEDIUM items resolved. See commit history for fixes.

## Executive Summary

| Dimension | Grade | Notes |
|-----------|-------|-------|
| `// SAFETY:` comments | **F** | Zero comments across entire project |
| Encapsulation (BurntSushi) | **A** | Public API is fully safe, unsafe hidden in internals |
| Usage justification (dtolnay) | **A** | All unsafe is FFI — no safe alternative exists |
| Concurrency ordering (Mara Bos) | **B** | Relaxed mostly acceptable, `_running` should be stronger |
| FFI specifics | **C** | 1 CRITICAL panic-across-FFI, other issues are hardening |

**Total unsafe blocks:** ~25 (across 5 files in macos-sys + 1 in app)

---

## Findings by Severity

### CRITICAL (1)

#### C1: Panic across FFI boundary in `event_tap_callback`

**File:** `macos-sys/src/event_tap.rs:112-159`

The `extern "C" fn event_tap_callback` calls user-provided `(ctx.callback)(HotkeyEvent::Pressed)` at line 135 and `(ctx.callback)(HotkeyEvent::Released)` at line 150. The callback is a `Box<dyn Fn(HotkeyEvent) + Send>` — arbitrary user code. If the callback panics, the panic unwinds through the `extern "C"` boundary into macOS CoreGraphics runtime — this is **undefined behavior** per Rust spec.

This is the only confirmed reachable panic-across-FFI in the codebase. The user-provided closure is an opaque `dyn Fn` with no panic guarantee.

**Fix:** Wrap callback invocations in `std::panic::catch_unwind(AssertUnwindSafe(|| { ... }))`.

---

### HIGH (1)

#### H1: Unsound `unsafe impl Send for StatusBarHandle`

**File:** `macos-sys/src/status_bar.rs:101`

`StatusBarHandle` wraps a raw pointer to `NSStatusItem`. NSStatusItem is **not thread-safe** — it must be accessed from the main thread only (AppKit requirement). Marking it `Send` allows:
- Moving it to another thread
- Calling `update_menu()` (`status_bar.rs:181`) from a background thread
- Calling `set_status_bar_icon()` (`status_bar.rs:188`) from a background thread
- `Drop::drop` calling `removeStatusItem:` from a non-main thread

**Current mitigation:** In practice, the handle lives in `Inner` which stays on Makepad's main thread. But the type system doesn't enforce this.

**Fix:** Remove `unsafe impl Send for StatusBarHandle`. This is the only fix that truly closes the boundary. A runtime thread assertion in Drop alone is insufficient — it doesn't protect `update_menu` and `set_status_bar_icon` calls. If callers need `Send`, use a main-thread proxy pattern (send commands via channel, execute on main thread).

---

### MEDIUM (7)

#### M1: `menu_action` lacks `catch_unwind` (defensive hardening)

**File:** `macos-sys/src/status_bar.rs:57-68`

`extern "C" fn menu_action` is registered as an ObjC method. The body only does `msg_send![sender, tag]` (returns 0 on nil — safe by ObjC convention), `try_send` (returns `Err` on failure, won't panic), and file I/O (can panic on allocation failure — extremely unlikely).

**Assessment:** Unlike C1, there is no user-provided closure here. The realistic panic surface is near-zero. This is a **defensive hardening** recommendation, not a confirmed UB path. Downgraded from HIGH to MEDIUM.

**Fix:** Wrap in `catch_unwind` as defense-in-depth. Low priority.

#### M2: `user_info` null check (defensive hardening)

**File:** `macos-sys/src/event_tap.rs:119`

```rust
let ctx = &*(user_info as *const TapContext);
```

The `user_info` pointer is created by our own code (`Box::into_raw(ctx)` at line 200) and passed to `CGEventTapCreate` (line 207). macOS passes it through unmodified to the callback. There is **no demonstrated path** where `user_info` is null.

**Assessment:** This is a defensive coding recommendation, not a proven UB path. Downgraded from HIGH to MEDIUM. Adding a null check is cheap and good practice, but not a blocking issue.

**Fix:** Add `if user_info.is_null() { return event; }` — cheap, zero risk.

#### M3: `static mut TARGET_CLASS` and `static mut TARGET`

**File:** `macos-sys/src/status_bar.rs:49, 84`

`static mut` guarded by `Once`. Currently sound because `call_once` ensures single initialization and reads happen after. However, `static mut` is being deprecated. Should migrate to `std::sync::OnceLock`.

#### M4: `Mutex::lock()` in event tap callback

**File:** `macos-sys/src/event_tap.rs:132`

`ctx.press_time.lock()` can block if poisoned or contended. In a CGEvent tap callback, blocking stalls the entire macOS input event pipeline. Should use `try_lock()`.

#### M5: `Box::into_raw` lifetime management

**File:** `macos-sys/src/event_tap.rs:195`

`ctx_ptr` is reclaimed via `Box::from_raw` only if `CFRunLoopRun()` returns normally. If the thread panics past `CFRunLoopRun`, `ctx_ptr` leaks. Both early-return paths (lines 215, 228) do clean up correctly.

#### M6: `CFRunLoopStop` with potentially stale pointer

**File:** `macos-sys/src/event_tap.rs:268`

The RunLoop ref is stored as `usize`. If the thread has already exited and cleaned up the RunLoop, calling `CFRunLoopStop` on a dangling pointer is UB. Mitigated by `_running` flag, but no synchronization guarantee.

#### M7: `CStr::from_ptr` on `UTF8String` return value

**Files:** `macos-sys/src/clipboard.rs:27`, `macos-sys/src/input_source.rs:38`

`UTF8String` returns a pointer to NSString's internal buffer, valid only while the NSString (or its autorelease pool) is alive. Currently safe within function scope, but fragile — an autorelease pool drain between `UTF8String` and `CStr::from_ptr` would be use-after-free.

---

### LOW (7)

| # | File | Issue |
|---|------|-------|
| L1 | event_tap.rs:126 | Debug file I/O in CGEvent callback — blocks input pipeline |
| L2 | event_tap.rs:252 | Thread handle not joined in Drop |
| L3 | status_bar.rs:61 | Debug file I/O in ObjC callback |
| L4 | clipboard.rs:9 | str_to_nsstring autoreleased — minor concern on background threads |
| L5 | input_source.rs:21 | Same autorelease concern |
| L6 | key_inject.rs:34 | `thread::sleep(10ms)` blocks caller — acceptable for utility |
| L7 | `_running` Ordering Relaxed | Should be Release/Acquire for cross-thread visibility, but not a practical bug since CFRunLoopStop is the actual stop mechanism |

---

## Concurrency Ordering Analysis

Per Mara Bos (《Rust Atomics and Locks》):

| Variable | Location | Ordering | Verdict |
|----------|----------|----------|---------|
| `was_pressed` (AtomicBool) | event_tap.rs | Relaxed | **OK** — single-thread CGEvent callback |
| `_running` (AtomicBool) | event_tap.rs | Relaxed | **Acceptable** — CFRunLoopStop is the actual stop mechanism, not the flag |
| `active` (AtomicBool) | audio.rs | Relaxed | **OK** — losing one frame is acceptable |
| `rms` (AtomicU64) | audio.rs | Relaxed | **OK** — approximate value, stale reads harmless |
| `device_sample_rate` (AtomicU64) | audio.rs | Relaxed | **OK** — set once early, read many times |

---

## FFI Checklist (per UNSAFE-REVIEW.md)

| Check | Status |
|-------|--------|
| `#[repr(C)]` for cross-FFI structs | ✅ `NSRect` in main.rs |
| Null pointer checks before dereference | ⚠️ `user_info` — defensive hardening (MEDIUM) |
| `CStr`/`CString` for string conversion | ✅ Used correctly |
| `extern "C"` ABI on all callbacks | ✅ All callbacks declared correctly |
| Panic cannot cross FFI boundary | ❌ **CRITICAL** — `event_tap_callback` executes user closure |
| `cbindgen`/`bindgen` for auto-generated bindings | N/A — using `makepad_objc_sys::msg_send!` macro |

---

## Required Actions (Priority Order)

### Must Fix Before Merge

1. **Add `catch_unwind` to `event_tap_callback`** (C1) — user-provided closure can panic
2. **Remove `unsafe impl Send for StatusBarHandle`** (H1) — or use main-thread proxy pattern

### Should Fix Soon

3. Replace `static mut` with `OnceLock` (M3)
4. Use `try_lock` instead of `lock` in event tap callback (M4)
5. Remove debug file I/O from FFI callbacks (L1, L3)
6. Add `// SAFETY:` comments to all unsafe blocks (project-wide)
7. Add `user_info` null check as defensive hardening (M2)
8. Add `catch_unwind` to `menu_action` as defense-in-depth (M1)

### Nice to Have

9. Join thread in `HotkeyHandle::drop` (L2)
10. Strengthen `_running` ordering to Release/Acquire (L7)

---

## Review History

| Date | Reviewer | Action |
|------|----------|--------|
| 2026-04-06 | Claude Opus 4.6 | Initial audit |
| 2026-04-06 | Codex | Review of audit — corrected H1→M2 (user_info is self-created, defensive only), H2→M1 (menu_action has near-zero panic surface), H3 fix inadequacy (runtime assertion insufficient, must remove Send or proxy) |
