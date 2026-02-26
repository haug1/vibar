# Fix: Clean up bar windows and module resources on monitor disconnect

## Context

vibar creates one `ApplicationWindow` per connected monitor at startup. When a monitor is disconnected, GTK moves the orphaned window to another monitor instead of destroying it. This leaves zombie windows and leaks all module resources (background threads, GLib sources, file descriptors, IPC connections).

The fix has two parts: (A) close the window on monitor disconnect, and (B) ensure all module resources are cleaned up when the window is closed.

## Changes

### 1. Close window on monitor invalidate — `src/main.rs`

Connect to each `gdk::Monitor`'s `invalidate` signal in the per-monitor loop. When fired, call `window.close()`.

```rust
monitor.connect_invalidate({
    let window = window.clone();
    move |_monitor| {
        window.close();
    }
});
```

### 2. Stop GLib sources when widget is orphaned — all module files

Every GLib source callback (`timeout_add_local`, `timeout_add_seconds_local`, `unix_fd_add_local`) holds a strong clone of a widget. After `window.close()`, that widget has no root window. Add a check at the top of each callback:

```rust
if label.root().is_none() {
    return ControlFlow::Break;
}
```

Returning `ControlFlow::Break` deregisters the source, drops the closure, and releases the strong widget ref. This also drops any `mpsc::Receiver` captured in the closure, causing subsequent `sender.send()` calls in background threads to fail.

**Files (GLib timeout with widget clone):**
- `src/modules/clock.rs:106` — `timeout_add_seconds_local` closure captures `label`
- `src/modules/cpu.rs:147` — `timeout_add_local` closure captures `label`
- `src/modules/disk.rs:130` — `timeout_add_local` closure captures `label`
- `src/modules/memory.rs:116` — `timeout_add_local` closure captures `label`
- `src/modules/temperature.rs:214` — `timeout_add_local` closure captures `label`
- `src/modules/exec.rs:113` — `timeout_add_local` closure captures `label`
- `src/modules/playerctl/mod.rs:120` — `timeout_add_local` closure captures `root`
- `src/modules/pulseaudio.rs` — `timeout_add_local` closure (UI drain timer) captures label/root widgets
- `src/modules/tray/mod.rs:116` — `timeout_add_local` closure captures `container`

**Files (GLib fd source with widget clone):**
- `src/modules/sway/workspaces.rs:99` — `unix_fd_add_local` closure captures `container`
- `src/modules/sway/mode.rs:89` — `unix_fd_add_local` closure captures `label`
- `src/modules/sway/window.rs:98` — `unix_fd_add_local` closure captures `label`

### 3. Exit background threads on channel disconnect — Pattern A modules

Most modules use `let _ = sender.send(update);` which silently ignores a disconnected receiver. Change to exit the thread when the send fails:

```rust
// Before:
let _ = sender.send(update);
// After:
if sender.send(update).is_err() {
    return;
}
```

**Files:**
- `src/modules/cpu.rs:142` — `let _ = sender.send(update);`
- `src/modules/disk.rs:126` — `let _ = sender.send(text);`
- `src/modules/memory.rs:112` — `let _ = sender.send(text);`
- `src/modules/temperature.rs:210` — `let _ = sender.send(update);`
- `src/modules/playerctl/mod.rs` — check `sender.send()` in backend thread (in `backend.rs`)
- `src/modules/tray/mod.rs:108` — already has `if sender.send(...).is_err() { return; }` ✓

**Sway modules** (pattern B) already exit on `signal_tx.write_all(&[1]).is_err()` ✓

### 4. Exit background threads — Pattern C modules (battery, backlight)

Battery and backlight use `glib::SendWeakRef` + `main_context.spawn()` instead of mpsc channels. The background thread can't directly check if the widget is gone because `SendWeakRef::upgrade()` only works on the main thread.

**Fix:** Add a shared `Arc<AtomicBool>` alive flag. The dispatch function's async closure checks if the weak ref upgrade fails, and if so, sets the flag to `false`. The background thread loop checks this flag and exits.

```rust
// In build function:
let alive = Arc::new(AtomicBool::new(true));

// In dispatch function:
fn dispatch_battery_ui_update(
    main_context: &glib::MainContext,
    label_weak: &glib::SendWeakRef<Label>,
    alive: &Arc<AtomicBool>,
    update: BatteryUiUpdate,
) {
    let label_weak = label_weak.clone();
    let alive = Arc::clone(alive);
    std::mem::drop(main_context.spawn(async move {
        match label_weak.upgrade() {
            Some(label) => apply_battery_ui_update(&label, &update),
            None => alive.store(false, Ordering::Relaxed),
        }
    }));
}

// In backend loop:
loop {
    if !alive.load(Ordering::Relaxed) {
        return;
    }
    // ... existing loop body
}
```

**Files:**
- `src/modules/battery.rs` — `run_battery_backend_loop` + `dispatch_battery_ui_update` + `build_battery_module`
- `src/modules/backlight.rs` — `run_backlight_backend_loop` + `dispatch_backlight_ui_update` + `build_backlight_module`

### 5. Exec module shared backend cleanup — `src/modules/exec.rs`

The exec module uses a shared backend (`SharedExecBackend`) with subscriber list. When a GLib timeout breaks (step 2), the `Receiver` is dropped, causing the subscriber's `Sender` to fail. The `broadcast()` method already retains only working senders (line 208: `.retain(|sender| sender.send(text.clone()).is_ok())`), so dead subscribers are automatically pruned. ✓

No additional changes needed for exec subscriber cleanup.

### 6. Pulseaudio module — `src/modules/pulseaudio.rs`

Read the full file to find the exact GLib source and thread pattern, then apply the same root-check + send-error-exit fixes.

## Verification

1. `make ci` — run the project's CI checks (build, test, lint)
2. Manual testing: run vibar on multi-monitor setup, disconnect a monitor, verify the bar window disappears and no orphaned threads remain
