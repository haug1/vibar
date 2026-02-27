# Codebase Improvements Plan

Findings from a full codebase audit. Ordered by impact-to-effort ratio.

## 1. Add release profile to Cargo.toml

**Impact:** High — reduces binary size and improves runtime performance with zero code changes.

**Current state:** No `[profile.release]` section. Release binary is 6.6M with default settings.

**Change:** Add to `Cargo.toml`:

```toml
[profile.release]
lto = true
codegen-units = 1
strip = true
```

- `lto = true` — enables link-time optimization across all crates (better inlining, dead code elimination)
- `codegen-units = 1` — required for full LTO, allows global optimization at cost of compile time
- `strip = true` — strips debug symbols from the binary

**Expected result:** Binary size reduction of ~40-60%. Slight runtime improvement from LTO inlining.

**Effort:** 1 minute. Add 4 lines.

---

## 2. Replace `df` process spawn with `libc::statvfs` in disk module

**Impact:** Medium — eliminates a process spawn + exec + stdout parse on every poll cycle (default every 30s, per bar window).

**Current state:** `src/modules/disk.rs:185-203` spawns `Command::new("df").arg("-B1").arg("-P").arg(path)` each poll, parses stdout lines, handles stderr.

**Change:** Replace `read_disk_status()` with a direct `libc::statvfs` call:

```rust
fn read_disk_status(path: &str) -> Result<DiskStatus, String> {
    let c_path = std::ffi::CString::new(path)
        .map_err(|_| "invalid path".to_string())?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let ret = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };
    if ret != 0 {
        return Err(format!("statvfs failed: {}", std::io::Error::last_os_error()));
    }
    let block = stat.f_frsize as u64;
    let total = stat.f_blocks * block;
    let free = stat.f_bavail * block;
    let used = total - (stat.f_bfree * block);
    Ok(DiskStatus {
        path: path.to_string(),
        total,
        used,
        free,
    })
}
```

This eliminates:
- `fork()` + `exec()` of `/usr/bin/df` every 30s
- stdout/stderr string allocation and parsing
- Error handling for process spawn failure

`libc` is already a dependency. The `parse_df_output` function and its tests can be removed.

**Effort:** Small. Replace one function, remove parser + parser tests, verify with `make ci`.

---

## 3. Return `&str` from icon selection helpers instead of cloning

**Impact:** Low-medium — eliminates a `String` clone on every poll cycle in three modules.

**Current state:** Three modules have nearly identical `icon_for_*` functions that return `String` by cloning from a `Vec<String>`:

- `src/modules/battery.rs:545-556` — `icon_for_capacity(&[String], u8) -> String`
- `src/modules/backlight.rs:743-753` — same pattern
- `src/modules/temperature.rs:349-359` — same pattern

Each calls `format_icons[index].clone()` on every poll to produce an owned `String`, which is then passed as `&icon` to `render_markup_template`.

**Change:** Return `&str` instead of `String`:

```rust
fn icon_for_capacity(format_icons: &[String], capacity: u8) -> &str {
    if format_icons.is_empty() {
        return "";
    }
    if format_icons.len() == 1 {
        return &format_icons[0];
    }
    let clamped = capacity.min(100) as usize;
    let index = (clamped * (format_icons.len() - 1)) / 100;
    &format_icons[index]
}
```

Apply the same change to all three modules. The callers already use the result as `&str` in `render_markup_template` replacements, so no downstream changes needed.

**Effort:** Small. Change return type and remove `.clone()` in three files.

---

## 4. Deduplicate the icon selection function

**Impact:** Low — reduces code duplication across three modules (battery, backlight, temperature).

**Current state:** The icon-for-percentage logic is copy-pasted in three places with identical implementations. The only difference is the function name and the input type (`u8` for battery/temperature, `f64` percentage for backlight that's pre-clamped to `u8` range).

**Change:** After fixing item 3, extract a shared helper in `src/modules/mod.rs`:

```rust
pub(crate) fn icon_for_percentage(format_icons: &[String], percent: u8) -> &str {
    if format_icons.is_empty() {
        return "";
    }
    if format_icons.len() == 1 {
        return &format_icons[0];
    }
    let clamped = percent.min(100) as usize;
    let index = (clamped * (format_icons.len() - 1)) / 100;
    &format_icons[index]
}
```

Then call `icon_for_percentage(&format_icons, capacity)` from battery, backlight, and temperature. The pulseaudio module has a different icon selection pattern (keyed by device type, not percentage) so it stays separate.

**Effort:** Small. Extract function, update three call sites, remove three duplicate functions.
