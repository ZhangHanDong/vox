# Unsafe Rust Audit Report — Vox

> **Date:** 2026-04-06
> **Auditor:** Claude Opus 4.6
> **Methodology:** Per UNSAFE-REVIEW.md (Ralf Jung / BurntSushi / dtolnay / Mara Bos)
> **Branch:** feat/meeting-minutes (commit 17efe49)

## Executive Summary

| Dimension | Grade | Notes |
|-----------|-------|-------|
| `// SAFETY:` comments | **F** | Zero comments across entire project |
| Encapsulation (BurntSushi) | **A** | Public API is fully safe, unsafe hidden in internals |
| Usage justification (dtolnay) | **A** | All unsafe is FFI — no safe alternative exists |
| Concurrency ordering (Mara Bos) | **B** | Relaxed mostly acceptable, `_running` should be stronger |
| FFI specifics | **C** | 2 CRITICAL panic-across-FFI, 1 HIGH null check missing |

**Total unsafe blocks:** ~25 (across 5 files in macos-sys + 1 in app)

---

## Findings by Severity

### CRITICAL (1)

#### C1: Panic across FFI boundary in `event_tap_callback`

**File:** `macos-sys/src/event_tap.rs:112-159`

The `extern "C" fn event_tap_callback` calls user-provided `(ctx.callback)(HotkeyEvent::Pressed)` at line 135 and `(ctx.callback)(HotkeyEvent::Released)` at line 150. If the callback panics, the panic unwinds through the `extern "C"` boundary into macOS CoreGraphics runtime — this is **undefined behavior** per Rust spec.

**Fix:** Wrap callback invocations in `std::panic::catch_unwind(AssertUnwindSafe(|| { ... }))`.

---

### HIGH (3)

#### H1: No null check on `user_info` in `event_tap_callback`

**File:** `macos-sys/src/event_tap.rs:119`

```rust
let ctx = &*(user_info as *const TapContext);
```

If macOS ever passes null `user_info`, this is immediate UB (null pointer dereference). In practice the OS passes through what we gave it, but defensive coding requires a null check.

**Fix:** Add `if user_info.is_null() { return event; }` before the cast.

#### H2: Panic across FFI in `menu_action`

**File:** `macos-sys/src/status_bar.rs:57-68`

Same issue as C1 — `extern "C" fn menu_action` registered as ObjC method selector. If `tx.try_send()` or file I/O panics, undefined behavior.

**Fix:** Wrap body in `catch_unwind`.

#### H3: Unsound `unsafe impl Send for StatusBarHandle`

**File:** `macos-sys/src/status_bar.rs:101`

`StatusBarHandle` wraps a raw pointer to `NSStatusItem`. NSStatusItem is **not thread-safe** — it must be accessed from the main thread only (AppKit requirement). Marking it `Send` allows moving it to another thread, violating AppKit's threading model.

**Risk:** If `StatusBarHandle` is dropped on a background thread, `removeStatusItem:` and `release` are called from the wrong thread — potential crash.

**Current mitigation:** In practice, the handle lives in `Inner` which stays on the main thread. But the type system doesn't enforce this.

**Fix:** Either remove `Send` impl, or add a runtime thread assertion in Drop.

---

### MEDIUM (7)

#### M1: `static mut TARGET_CLASS` and `static mut TARGET`

**File:** `macos-sys/src/status_bar.rs:49, 84`

`static mut` guarded by `Once`. Currently sound because `call_once` ensures single initialization and reads happen after. However, `static mut` is being deprecated. Should migrate to `std::sync::OnceLock`.

#### M2: `Mutex::lock()` in event tap callback

**File:** `macos-sys/src/event_tap.rs:132`

`ctx.press_time.lock()` can block if poisoned or contended. In a CGEvent tap callback, blocking stalls the entire macOS input event pipeline. Should use `try_lock()`.

#### M3: `Box::into_raw` lifetime management

**File:** `macos-sys/src/event_tap.rs:195`

`ctx_ptr` is reclaimed via `Box::from_raw` only if `CFRunLoopRun()` returns normally. If the thread panics past `CFRunLoopRun`, `ctx_ptr` leaks. Both early-return paths (lines 215, 228) do clean up correctly.

#### M4: `CFRunLoopStop` with potentially stale pointer

**File:** `macos-sys/src/event_tap.rs:268`

The RunLoop ref is stored as `usize`. If the thread has already exited and cleaned up the RunLoop, calling `CFRunLoopStop` on a dangling pointer is UB. Mitigated by `_running` flag, but no synchronization guarantee.

#### M5: `CStr::from_ptr` on `UTF8String` return value

**Files:** `macos-sys/src/clipboard.rs:27`, `macos-sys/src/input_source.rs:38`

`UTF8String` returns a pointer to NSString's internal buffer, valid only while the NSString (or its autorelease pool) is alive. Currently safe within function scope, but fragile — an autorelease pool drain between `UTF8String` and `CStr::from_ptr` would be use-after-free.

#### M6: `event_tap_callback` debug file I/O

**File:** `macos-sys/src/event_tap.rs:126-128`

`std::fs::OpenOptions::new()...open()` allocates and may block. Inappropriate in an event tap callback that can stall input processing. Should be removed in production.

#### M7: `_running` Ordering too weak

**File:** `macos-sys/src/event_tap.rs:264`

Drop stores `false` with `Relaxed`, but `CFRunLoopStop` depends on this being visible. Should use `Release` (store) / `Acquire` (load) pair. Currently not a practical bug because `CFRunLoopStop` directly interrupts the RunLoop regardless.

---

### LOW (7)

| # | File | Issue |
|---|------|-------|
| L1 | event_tap.rs:126 | Debug file I/O in CGEvent callback |
| L2 | event_tap.rs:252 | Thread handle not joined in Drop |
| L3 | status_bar.rs:61 | Debug file I/O in ObjC callback |
| L4 | status_bar.rs:136 | NSMenu `new` ownership — actually correct (AppKit retains on setMenu:) |
| L5 | clipboard.rs:9 | str_to_nsstring autoreleased — OK on main thread, minor concern on background |
| L6 | input_source.rs:21 | Same autorelease concern |
| L7 | key_inject.rs:34 | `thread::sleep(10ms)` in simulate_cmd_v — blocks caller, acceptable for utility |

---

## Concurrency Ordering Analysis

Per Mara Bos (《Rust Atomics and Locks》):

| Variable | Location | Ordering | Verdict |
|----------|----------|----------|---------|
| `was_pressed` (AtomicBool) | event_tap.rs | Relaxed | **OK** — single-thread CGEvent callback |
| `_running` (AtomicBool) | event_tap.rs | Relaxed | **Should be Release/Acquire** — cross-thread stop signal |
| `active` (AtomicBool) | audio.rs | Relaxed | **OK** — losing one frame is acceptable |
| `rms` (AtomicU64) | audio.rs | Relaxed | **OK** — approximate value, stale reads harmless |
| `device_sample_rate` (AtomicU64) | audio.rs | Relaxed | **OK** — set once early, read many times |

---

## FFI Checklist (per UNSAFE-REVIEW.md)

| Check | Status |
|-------|--------|
| `#[repr(C)]` for cross-FFI structs | ✅ `NSRect` in main.rs |
| Null pointer checks before dereference | ❌ Missing on `user_info` in event_tap_callback |
| `CStr`/`CString` for string conversion | ✅ Used correctly |
| `extern "C"` ABI on all callbacks | ✅ All callbacks declared correctly |
| Panic cannot cross FFI boundary | ❌ **CRITICAL** — 2 callbacks lack `catch_unwind` |
| `cbindgen`/`bindgen` for auto-generated bindings | N/A — using `makepad_objc_sys::msg_send!` macro |

---

## Required Actions (Priority Order)

### Must Fix Before Merge

1. **Add `catch_unwind` to `event_tap_callback`** (C1)
2. **Add `catch_unwind` to `menu_action`** (H2)
3. **Add null check on `user_info`** (H1)

### Should Fix Soon

4. Replace `static mut` with `OnceLock` (M1)
5. Use `try_lock` instead of `lock` in event tap callback (M2)
6. Remove debug file I/O from FFI callbacks (M6, L1, L3)
7. Add `// SAFETY:` comments to all unsafe blocks (project-wide)

### Nice to Have

8. Evaluate removing `Send` from `StatusBarHandle` (H3)
9. Strengthen `_running` ordering to Release/Acquire (M7)
10. Join thread in `HotkeyHandle::drop` (L2)
