#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <version>" >&2
  echo "Example: $0 v0.3.2" >&2
  exit 1
fi

VERSION_RAW="$1"
if [[ ! "${VERSION_RAW}" =~ ^[A-Za-z0-9._-]+$ ]]; then
  echo "Invalid version '${VERSION_RAW}'. Allowed: letters, numbers, dot, underscore, hyphen." >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${ROOT_DIR}/docs/demo-${VERSION_RAW}"
VENV_DIR="/tmp/navix-demo-venv"
PID_FILE="/tmp/navix-demo-generator.pids"

required_commands=(ghostty xwininfo ffmpeg python3 pgrep)
for cmd in "${required_commands[@]}"; do
  if ! command -v "${cmd}" >/dev/null 2>&1; then
    echo "Missing required command: ${cmd}" >&2
    exit 1
  fi
done

cleanup_stale_script_pids() {
  local pid kind comm
  [[ -f "${PID_FILE}" ]] || return 0
  while read -r kind pid; do
    [[ -n "${kind}" && -n "${pid}" ]] || continue
    [[ "${pid}" =~ ^[0-9]+$ ]] || continue
    comm="$(ps -p "${pid}" -o comm= 2>/dev/null | tr -d '[:space:]' || true)"
    case "${kind}" in
      navix)
        [[ "${comm}" == "navix" ]] || continue
        ;;
      ghostty)
        [[ "${comm}" == "ghostty" ]] || continue
        ;;
      *)
        continue
        ;;
    esac
    kill -TERM "${pid}" 2>/dev/null || true
    sleep 0.05
    kill -KILL "${pid}" 2>/dev/null || true
  done < "${PID_FILE}"
  rm -f "${PID_FILE}"
}

cleanup_stale_script_pids

if ! "${ROOT_DIR}/bin/navix" --version >/dev/null 2>&1; then
  echo "Failed to build/run navix via ./bin/navix --version" >&2
  exit 1
fi

if [[ -x "${ROOT_DIR}/.navix-build/debug/navix" && \
      ( ! -x "${ROOT_DIR}/target/debug/navix" || "${ROOT_DIR}/.navix-build/debug/navix" -nt "${ROOT_DIR}/target/debug/navix" ) ]]; then
  NAVIX_BIN="${ROOT_DIR}/.navix-build/debug/navix"
elif [[ -x "${ROOT_DIR}/target/debug/navix" ]]; then
  NAVIX_BIN="${ROOT_DIR}/target/debug/navix"
else
  echo "Could not locate a built navix binary." >&2
  exit 1
fi

mkdir -p "${OUT_DIR}"

if [[ ! -x "${VENV_DIR}/bin/python" ]]; then
  python3 -m venv "${VENV_DIR}"
fi
"${VENV_DIR}/bin/pip" install --quiet --disable-pip-version-check pillow

export NAVIX_DEMO_ROOT="${ROOT_DIR}"
export NAVIX_DEMO_OUT="${OUT_DIR}"
export NAVIX_DEMO_BIN="${NAVIX_BIN}"
export NAVIX_DEMO_VERSION="${VERSION_RAW}"
export NAVIX_DEMO_PID_FILE="${PID_FILE}"
export DISPLAY="${DISPLAY:-:1}"

"${VENV_DIR}/bin/python" <<'PY'
import colorsys
import os
import re
import signal
import subprocess
import time
from pathlib import Path

from PIL import Image, ImageChops, ImageSequence, ImageStat

ROOT = Path(os.environ["NAVIX_DEMO_ROOT"])
OUT = Path(os.environ["NAVIX_DEMO_OUT"])
NAVIX_BIN = os.environ["NAVIX_DEMO_BIN"]
VERSION = os.environ["NAVIX_DEMO_VERSION"]
DISPLAY_STR = os.environ.get("DISPLAY", ":1")
PID_FILE = Path(os.environ["NAVIX_DEMO_PID_FILE"])
STARTED_NAVIX_PIDS = set()
STARTED_GHOSTTY_PROCS = []


def asset_name(kind: str, ext: str = "png") -> str:
    return f"navix-{VERSION}-{kind}.{ext}"


def run(cmd):
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def out(cmd):
    return subprocess.check_output(cmd, text=True, stderr=subprocess.DEVNULL)


def ghostty_windows():
    tree = out(["xwininfo", "-root", "-tree"])
    windows = []
    for line in tree.splitlines():
        if '"ghostty" "com.mitchellh.ghostty"' not in line:
            continue
        m_id = re.search(r"(0x[0-9a-f]+)", line)
        m_geo = re.search(r"(\d+)x(\d+)\+(\d+)\+(\d+)", line)
        if not m_id or not m_geo:
            continue
        width = int(m_geo.group(1))
        height = int(m_geo.group(2))
        if width >= 800 and height >= 500:
            windows.append(int(m_id.group(1), 16))
    return set(windows)


def navix_pids():
    try:
        return {int(v) for v in out(["pgrep", "-x", "navix"]).split() if v.strip()}
    except subprocess.CalledProcessError:
        return set()


def latest_navix_pid():
    try:
        return int(out(["pgrep", "-n", "-x", "navix"]).strip())
    except Exception:
        return None


def persist_pid_file():
    lines = []
    for pid in sorted(STARTED_NAVIX_PIDS):
        lines.append(f"navix {pid}")
    for proc in STARTED_GHOSTTY_PROCS:
        if proc.pid and proc.poll() is None:
            lines.append(f"ghostty {proc.pid}")
    if lines:
        PID_FILE.write_text("\n".join(lines) + "\n")
    elif PID_FILE.exists():
        PID_FILE.unlink()


def register_navix_pid(pid):
    STARTED_NAVIX_PIDS.add(pid)
    persist_pid_file()


def unregister_navix_pid(pid):
    STARTED_NAVIX_PIDS.discard(pid)
    persist_pid_file()


def register_ghostty_proc(proc):
    STARTED_GHOSTTY_PROCS.append(proc)
    persist_pid_file()


def unregister_ghostty_proc(proc):
    if proc in STARTED_GHOSTTY_PROCS:
        STARTED_GHOSTTY_PROCS.remove(proc)
    persist_pid_file()


def pid_is_navix(pid):
    try:
        return out(["ps", "-p", str(pid), "-o", "comm="]).strip() == "navix"
    except Exception:
        return False


def window_geometry(window_id):
    info = out(["xwininfo", "-id", hex(window_id)])
    x = y = width = height = None
    for line in info.splitlines():
        line = line.strip()
        if line.startswith("Absolute upper-left X:"):
            x = int(line.split(":", 1)[1].strip())
        elif line.startswith("Absolute upper-left Y:"):
            y = int(line.split(":", 1)[1].strip())
        elif line.startswith("Width:"):
            width = int(line.split(":", 1)[1].strip())
        elif line.startswith("Height:"):
            height = int(line.split(":", 1)[1].strip())
    if None in (x, y, width, height):
        raise RuntimeError("Could not parse Ghostty window geometry")
    return x, y, width, height


def display_geometry():
    info = out(["xwininfo", "-root"])
    width = height = None
    for line in info.splitlines():
        line = line.strip()
        if line.startswith("Width:"):
            width = int(line.split(":", 1)[1].strip())
        elif line.startswith("Height:"):
            height = int(line.split(":", 1)[1].strip())
    if None in (width, height):
        raise RuntimeError("Could not parse root display geometry")
    return width, height


def send_bytes(pid, payload, pause=0.15):
    with open(f"/proc/{pid}/fd/0", "wb", buffering=0) as fd:
        fd.write(payload)
    time.sleep(pause)


def metric(image):
    rgb = image.convert("RGB")
    pixels = rgb.tobytes()
    total = len(pixels) // 3
    step = max(1, total // 90000)
    yellow = blue = 0
    diff = 0
    samples = 0
    width, _ = rgb.size
    for i in range(0, total - width - 1, step):
        base = i * 3
        r = pixels[base]
        g = pixels[base + 1]
        b = pixels[base + 2]
        h, s, v = colorsys.rgb_to_hsv(r / 255.0, g / 255.0, b / 255.0)
        h *= 360
        s *= 255
        v *= 255
        if 35 <= h <= 80 and s >= 45 and v >= 45:
            yellow += 1
        if 180 <= h <= 260 and s >= 45 and v >= 30:
            blue += 1
        base2 = (i + 1) * 3
        r2 = pixels[base2]
        g2 = pixels[base2 + 1]
        b2 = pixels[base2 + 2]
        base3 = (i + width) * 3
        r3 = pixels[base3]
        g3 = pixels[base3 + 1]
        b3 = pixels[base3 + 2]
        diff += abs(r - r2) + abs(g - g2) + abs(b - b2)
        diff += abs(r - r3) + abs(g - g3) + abs(b - b3)
        samples += 1
    edge = diff / max(1, samples)
    unique_step = max(1, total // 140000)
    unique = len(
        {
            (
                pixels[idx * 3],
                pixels[idx * 3 + 1],
                pixels[idx * 3 + 2],
            )
            for idx in range(0, total, unique_step)
        }
    )
    return yellow, blue, edge, unique


def is_valid_image(image):
    y, b, edge, unique = metric(image)
    return y > 8 and b > 80 and edge > 8 and unique > 120, (y, b, edge, unique)


def grab_png(path, x, y, width, height):
    run(
        [
            "ffmpeg",
            "-y",
            "-f",
            "x11grab",
            "-video_size",
            f"{width}x{height}",
            "-i",
            f"{DISPLAY_STR}+{x},{y}",
            "-frames:v",
            "1",
            str(path),
        ]
    )


def capture_valid_png(path, x, y, width, height, tries=14):
    last = (0, 0, 0, 0)
    for _ in range(tries):
        grab_png(path, x, y, width, height)
        ok, vals = is_valid_image(Image.open(path))
        last = vals
        if ok:
            return vals
        time.sleep(0.25)
    raise RuntimeError(f"Capture validation failed for {path.name}: {last}")


def launch_navix(mode, startup_overlay=None, attempts=4):
    for _ in range(attempts):
        before_windows = ghostty_windows()
        before_pids = navix_pids()
        startup_overlay_value = ""
        if startup_overlay in ("help", "config"):
            startup_overlay_value = startup_overlay
        cmd = (
            "unset NO_COLOR; "
            "export TERM=xterm-ghostty; "
            "export COLORTERM=truecolor; "
            "export NAVIX_LAUNCH_SHELL=/usr/bin/zsh; "
            f"export NAVIX_STARTUP_OVERLAY='{startup_overlay_value}'; "
            f"\"{NAVIX_BIN}\" --path ~/repos/navix {mode}"
        )
        ghostty = subprocess.Popen(
            [
                "ghostty",
                "--gtk-single-instance=false",
                "--window-width=999",
                "--window-height=999",
                "-e",
                "bash",
                "-lc",
                cmd,
            ],
            cwd=str(ROOT),
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        register_ghostty_proc(ghostty)
        window_id = pid = None
        for _ in range(340):
            time.sleep(0.1)
            new_windows = ghostty_windows() - before_windows
            newest_pid = latest_navix_pid()
            if new_windows and newest_pid and newest_pid not in before_pids:
                window_id = sorted(new_windows)[-1]
                pid = newest_pid
                break
        if window_id and pid:
            register_navix_pid(pid)
            return ghostty, pid, window_id, window_geometry(window_id)
        ghostty.terminate()
        try:
            ghostty.wait(timeout=2)
        except subprocess.TimeoutExpired:
            ghostty.kill()
            try:
                ghostty.wait(timeout=1)
            except subprocess.TimeoutExpired:
                pass
        unregister_ghostty_proc(ghostty)
        leaked = navix_pids() - before_pids
        for leaked_pid in leaked:
            register_navix_pid(leaked_pid)
    raise RuntimeError(f"Failed to launch navix ({mode})")


def close_navix(ghostty, pid):
    try:
        if pid and pid_is_navix(pid):
            send_bytes(pid, b"\x04", pause=0.05)
    except OSError:
        pass
    try:
        ghostty.wait(timeout=4)
    except subprocess.TimeoutExpired:
        ghostty.terminate()
        try:
            ghostty.wait(timeout=2)
        except subprocess.TimeoutExpired:
            ghostty.kill()
            try:
                ghostty.wait(timeout=1)
            except subprocess.TimeoutExpired:
                pass
    if pid:
        unregister_navix_pid(pid)
    unregister_ghostty_proc(ghostty)


def cleanup_leaked_processes():
    # First terminate any tracked navix children that may still be alive.
    for pid in list(STARTED_NAVIX_PIDS):
        if not pid_is_navix(pid):
            STARTED_NAVIX_PIDS.discard(pid)
            continue
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            STARTED_NAVIX_PIDS.discard(pid)
            continue
    time.sleep(0.2)
    for pid in list(STARTED_NAVIX_PIDS):
        if not pid_is_navix(pid):
            STARTED_NAVIX_PIDS.discard(pid)
            continue
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        unregister_navix_pid(pid)

    # Then ensure tracked Ghostty processes are gone.
    for proc in list(STARTED_GHOSTTY_PROCS):
        if proc.poll() is not None:
            STARTED_GHOSTTY_PROCS.remove(proc)
            continue
        proc.terminate()
        try:
            proc.wait(timeout=2)
        except subprocess.TimeoutExpired:
            proc.kill()
            try:
                proc.wait(timeout=1)
            except subprocess.TimeoutExpired:
                pass
        unregister_ghostty_proc(proc)
    persist_pid_file()


captures = {}
ordered = [
    asset_name("navigation"),
    asset_name("help"),
    asset_name("config"),
    asset_name("preview"),
    asset_name("shell"),
]
gif_name = asset_name("demo", ext="gif")
display_w, display_h = display_geometry()
min_capture_w = max(800, int(display_w * 0.9))
min_capture_h = max(500, int(display_h * 0.82))


def capture_mode_frame(mode, name, startup_overlay=None, shortcut_bytes=None, attempts=4):
    last_error = None
    for _ in range(attempts):
        ghostty = None
        pid = None
        try:
            ghostty, pid, _wid, (x, y, w, h) = launch_navix(
                mode, startup_overlay=startup_overlay
            )
            if w < min_capture_w or h < min_capture_h:
                raise RuntimeError(
                    f"Window too small for capture: {w}x{h} (need at least {min_capture_w}x{min_capture_h})"
                )
            time.sleep(1.0)
            if shortcut_bytes is not None:
                send_bytes(pid, shortcut_bytes, pause=0.25)
                time.sleep(0.35)
            captures[name] = capture_valid_png(OUT / name, x, y, w, h)
            return
        except Exception as err:
            last_error = err
        finally:
            if ghostty is not None and pid is not None:
                close_navix(ghostty, pid)
    raise RuntimeError(f"Failed to capture {name}: {last_error}")


try:
    # Base pane captures
    for mode, name in [
        ("--navigation", asset_name("navigation")),
        ("--preview", asset_name("preview")),
        ("--shell", asset_name("shell")),
    ]:
        capture_mode_frame(mode, name)

    # Help overlay
    help_name = asset_name("help")
    capture_mode_frame("--navigation", help_name, startup_overlay="help")

    # Config overlay
    config_name = asset_name("config")
    capture_mode_frame("--navigation", config_name, startup_overlay="config")

    # Ensure overlays are visually different from base navigation
    navigation_name = asset_name("navigation")
    navigation_img = Image.open(OUT / navigation_name).convert("RGB")
    overlay_shortcuts = {
        help_name: b"\x1b?",
        config_name: b"\x1bc",
    }
    for overlay_name in [help_name, config_name]:
        diff = ImageChops.difference(
            navigation_img, Image.open(OUT / overlay_name).convert("RGB")
        )
        diff_mean = sum(ImageStat.Stat(diff).mean) / 3
        if diff_mean < 2.0:
            # Fallback for older navix binaries without NAVIX_STARTUP_OVERLAY support.
            capture_mode_frame(
                "--navigation",
                overlay_name,
                shortcut_bytes=overlay_shortcuts[overlay_name],
            )
            diff = ImageChops.difference(
                navigation_img,
                Image.open(OUT / overlay_name).convert("RGB"),
            )
            diff_mean = sum(ImageStat.Stat(diff).mean) / 3
        if diff_mean < 2.0:
            raise RuntimeError(
                f"{overlay_name} appears too similar to navigation (diff={diff_mean:.3f})"
            )

    # Build slideshow GIF from verified PNGs
    frames = [Image.open(OUT / name).convert("P", palette=Image.ADAPTIVE, colors=256) for name in ordered]
    durations = [1800, 1800, 1800, 1800, 1800]
    frames[0].save(
        OUT / gif_name,
        save_all=True,
        append_images=frames[1:],
        duration=durations,
        loop=0,
        optimize=False,
        disposal=2,
    )

    # Validate all GIF frames
    gif = Image.open(OUT / gif_name)
    bad_frames = 0
    for frame in ImageSequence.Iterator(gif):
        ok, _vals = is_valid_image(frame)
        if not ok:
            bad_frames += 1
    if bad_frames:
        raise RuntimeError(f"GIF validation failed: {bad_frames} bad frames")

    print("Generated demo assets:")
    for name in ordered + [gif_name]:
        print(f" - {OUT / name}")
    print("Metrics:")
    for name in ordered:
        y, b, edge, uniq = captures[name]
        print(f" - {name}: yellow={y} blue={b} edge={edge:.2f} unique={uniq}")
    print(f" - {gif_name}: frames={getattr(gif, 'n_frames', 1)} bad_frames={bad_frames}")
finally:
    cleanup_leaked_processes()
PY

echo "Done."
