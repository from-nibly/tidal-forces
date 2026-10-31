#!/usr/bin/env python3
"""Run the native tray contract on a fresh private bus, never the desktop bus."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

build = subprocess.run(
    ["cargo", "test", "--locked", "--bin", "tidal-forces", "--no-run", "--message-format=json"],
    check=True, text=True, stdout=subprocess.PIPE,
)
executables = []
for line in build.stdout.splitlines():
    message = json.loads(line)
    if (message.get("reason") == "compiler-artifact"
            and message.get("target", {}).get("name") == "tidal-forces"
            and message.get("profile", {}).get("test") and message.get("executable")):
        executables.append(message["executable"])
assert len(executables) == 1, "Expected exactly one application test binary"
root = Path(tempfile.mkdtemp(prefix="tidal-tray-test-", dir="/tmp")).resolve()
env = os.environ.copy()
for name in ["DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET", "DBUS_SESSION_BUS_PID", "DBUS_SESSION_BUS_WINDOWID"]:
    env.pop(name, None)
for name, folder in [("HOME", "home"), ("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"),
                     ("XDG_CACHE_HOME", "cache"), ("XDG_STATE_HOME", "state"), ("XDG_RUNTIME_DIR", "runtime")]:
    path = root / folder
    path.mkdir(mode=0o700)
    env[name] = str(path)
env["DBUS_SESSION_BUS_ADDRESS"] = f"unix:path={root}/bus"
env["DBUS_SYSTEM_BUS_ADDRESS"] = f"unix:path={root}/no-system-bus"
env["TIDAL_TEST_PRIVATE_TRAY"] = str(root)
env["TMPDIR"] = "/tmp"
with (root / "daemon.log").open("w") as log:
    daemon = subprocess.Popen(
        ["dbus-daemon", "--session", "--nofork", "--nopidfile", f"--address={env['DBUS_SESSION_BUS_ADDRESS']}"],
        env=env, stdout=subprocess.DEVNULL, stderr=log,
    )
    try:
        deadline = time.monotonic() + 5
        while not (root / "bus").exists():
            assert daemon.poll() is None and time.monotonic() < deadline, "Private bus did not start"
            time.sleep(0.02)
        subprocess.run([executables[0], "--exact", "tray::tests::native_tray_on_isolated_bus", "--ignored"], env=env, check=True, timeout=35)
        subprocess.run([executables[0], "ui::mini::tests::"], env=env, check=True, timeout=20)
    finally:
        daemon.terminate()
        try:
            daemon.wait(timeout=5)
        except subprocess.TimeoutExpired:
            daemon.kill()
            daemon.wait()
print(f"Private tray evidence: {root}")
