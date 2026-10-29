#!/usr/bin/env python3
"""Run only the output-recovery contract with private, non-audible ALSA output."""
import json
import os
import pathlib
import subprocess
import sys
import tempfile

os.umask(0o077)
if len(sys.argv) > 1:
    binary = pathlib.Path(sys.argv[1]).resolve()
else:
    build = subprocess.run(["cargo", "test", "--locked", "--no-run", "--message-format=json"], check=True, text=True, stdout=subprocess.PIPE)
    records = [json.loads(line) for line in build.stdout.splitlines()]
    binaries = [r["executable"] for r in records if r.get("reason") == "compiler-artifact" and r.get("profile", {}).get("test") and r.get("executable") and r["target"]["name"] == "tidal-forces"]
    assert len(binaries) == 1, "Expected one application test executable"
    binary = pathlib.Path(binaries[0]).resolve()
root = pathlib.Path(tempfile.mkdtemp(prefix="tidal-v030-null-", dir="/tmp"))
env = os.environ.copy()
for key in ("DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET", "HYPRLAND_INSTANCE_SIGNATURE", "DBUS_SESSION_BUS_ADDRESS"):
    env.pop(key, None)
for key in ("HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME", "XDG_RUNTIME_DIR"):
    path = root / key.lower()
    path.mkdir(mode=0o700)
    env[key] = str(path)
config = root / "alsa.conf"
config.write_text("pcm.!default { type tidal_test_missing_plugin }\n")
env.update(ALSA_CONFIG_PATH=str(config), TIDAL_TEST_ISOLATED_NULL_OUTPUT="1", TMPDIR=str(root))
with (root / "contract.log").open("w") as log:
    result = subprocess.run([
        "dbus-run-session", "--", str(binary),
        "--exact", "audio::next_tests::output_recovery_on_isolated_null_alsa",
        "--ignored", "--nocapture",
    ], env=env, stdout=log, stderr=log, timeout=40)
print("Isolated null-output evidence:", root)
raise SystemExit(result.returncode)
