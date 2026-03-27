# Recording the VHS demo (GIF and screenshots)

## Default (works everywhere)

[`scripts/demo.tape`](../scripts/demo.tape) launches **`./target/release/macjet` without `sudo`**. That avoids **[VHS](https://github.com/charmbracelet/vhs)** blocking on a `Password:` prompt (VHS uses its own PTY; macOS `sudo` does not reuse your outer-shell ticket).

From the repo root:

```bash
./scripts/record-demo.sh
```

Energy view (4) has **limited metrics** without root; the rest of the UI records normally. This is the path intended for **unattended** recording and CI-style checks.

The tape uses a **1680×900** terminal so the process list and inspector are not clipped. **`Set Padding 0`** turns off VHS’s default **60px** outer padding (otherwise you get a gray border around the recording). The in-app layout splits the body **3:2** (tree vs detail) so the right panel scales with terminal width.

### Colors look gray in the GIF?

MacJet uses 24‑bit RGB in the TUI. **[crossterm](https://github.com/crossterm-rs/crossterm)** turns **off** all styling when **`NO_COLOR`** is set to any non-empty value (see [no-color.org](https://no-color.org/)). If your shell exports `NO_COLOR`, VHS can inherit it and the recording looks monochrome. **`record-demo.sh`** unsets it and sets **`COLORTERM=truecolor`** / **`TERM=xterm-256color`** before running VHS; **`scripts/demo.tape`** also sets `Env NO_COLOR ""` and the same term vars for the recorded session. If you run `vhs` directly, use `env -u NO_COLOR COLORTERM=truecolor TERM=xterm-256color vhs scripts/demo.tape`.

## Optional: full energy / thermal in the recording

If you need `powermetrics` data in the GIF and PNGs:

1. Change the tape launch line to `sudo ./target/release/macjet` (or your absolute path).
2. On the recording machine only, configure **one** of:
   - **Global sudo timestamp:** `Defaults:YOUR_USERNAME timestamp_type=global` in `/etc/sudoers.d/` (via `visudo`), or
   - **Narrow NOPASSWD:** `YOUR_USERNAME ALL=(root) NOPASSWD: /absolute/path/to/target/release/macjet`

Do not commit sudoers files.

### Why `sudo -v && vhs` failed

`sudo -v` in your normal terminal does **not** apply inside VHS’s PTY when `tty_tickets` is on. Optional check after `sudo -v` in one terminal:

```bash
sudo -n true && echo "same ticket" || echo "needs own auth"
```

In a **new** terminal, you often see `needs own auth` — same class of issue as VHS.

## Output files

- `assets/macjet_demo.gif`
- `assets/view_apps.png`
- `assets/view_energy.png`
- `assets/view_reclaim.png`

### Size checks (`record-demo.sh`)

The script exits **non-zero** if outputs are missing or too small (e.g. stuck prompt or truncated run). Defaults: GIF **≥ 70,000** bytes, each PNG **≥ 12,000** bytes.

```bash
export VHS_DEMO_MIN_GIF_BYTES=90000
export VHS_DEMO_MIN_PNG_BYTES=15000
./scripts/record-demo.sh
```
