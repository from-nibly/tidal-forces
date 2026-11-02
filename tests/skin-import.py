#!/usr/bin/env python3
"""Exercise classic-skin persistence only inside a fresh private profile; no audio/account."""
import os
import json
import pathlib
import subprocess
import tempfile

os.umask(0o077)
build = subprocess.run(["cargo", "test", "--locked", "--no-run", "--message-format=json"], check=True, text=True, stdout=subprocess.PIPE)
records = [json.loads(line) for line in build.stdout.splitlines() if line.startswith("{")]
binaries = [r["executable"] for r in records if r.get("reason") == "compiler-artifact" and r.get("profile", {}).get("test") and r.get("executable") and r["target"]["name"] == "tidal-forces"]
assert len(binaries) == 1, "Expected one application test binary"
root = pathlib.Path(tempfile.mkdtemp(prefix="tidal-skin-test-", dir="/tmp"))
env = os.environ.copy()
env.setdefault("CARGO_HOME", str(pathlib.Path.home() / ".cargo"))
for key in list(env):
    if key.startswith("PULSE_") or key in ("WAYLAND_DISPLAY", "WAYLAND_SOCKET", "DISPLAY", "DBUS_SESSION_BUS_ADDRESS"):
        env.pop(key, None)
for key, part in [("HOME", "home"), ("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"), ("XDG_STATE_HOME", "state"), ("XDG_CACHE_HOME", "cache"), ("XDG_RUNTIME_DIR", "runtime")]:
    path = root / part
    path.mkdir(mode=0o700)
    env[key] = str(path)
alsa = root / "alsa.conf"
alsa.write_text("pcm.!default { type null }\n")
env.update(TIDAL_TEST_SKINS_ROOT=str(root), ALSA_CONFIG_PATH=str(alsa), PULSE_SERVER="unix:" + str(root / "no-live-output"), DBUS_SESSION_BUS_ADDRESS="unix:path=" + str(root / "no-session-bus"), DBUS_SYSTEM_BUS_ADDRESS="unix:path=" + str(root / "no-system-bus"))
subprocess.run([binaries[0], "ui::classic::tests::classic_skin_import_on_private_profile", "--ignored", "--exact"], env=env, check=True, timeout=30)
print("Private skin import contract:", root)
