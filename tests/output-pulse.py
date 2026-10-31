#!/usr/bin/env python3
"""Native routing contract against a private daemon with only two null sinks."""
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time

os.umask(0o077)
if len(sys.argv) > 1:
    binary = pathlib.Path(sys.argv[1]).resolve()
else:
    build = subprocess.run(["cargo", "test", "--locked", "--no-run", "--message-format=json"], check=True, text=True, stdout=subprocess.PIPE)
    records = [json.loads(line) for line in build.stdout.splitlines()]
    binaries = [r["executable"] for r in records if r.get("reason") == "compiler-artifact" and r.get("profile", {}).get("test") and r.get("executable") and r["target"]["name"] == "tidal-forces"]
    assert len(binaries) == 1
    binary = pathlib.Path(binaries[0]).resolve()
root = pathlib.Path(tempfile.mkdtemp(prefix="tidal-v031-pulse-", dir="/tmp"))
env = os.environ.copy()
for key in list(env):
    if key.startswith("PULSE_") or key in ("DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET", "HYPRLAND_INSTANCE_SIGNATURE", "DBUS_SESSION_BUS_ADDRESS", "DBUS_SYSTEM_BUS_ADDRESS"):
        env.pop(key, None)
for key in ("HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME", "XDG_RUNTIME_DIR"):
    path = root / key.lower()
    path.mkdir(mode=0o700)
    env[key] = str(path)
env.update(PULSE_SERVER=f"unix:{root}/native", TIDAL_TEST_ISOLATED_PULSE=str(root), TMPDIR=str(root), DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={root}/no-system-bus")
config = root / "null.pa"
config.write_text(f"load-module module-native-protocol-unix socket={root}/native auth-anonymous=1\nload-module module-null-sink sink_name=fixture_a\nload-module module-null-sink sink_name=fixture_b\nset-default-sink fixture_a\n")
with (root / "daemon.log").open("w") as daemon_log, (root / "contract.log").open("w") as log:
    daemon = subprocess.Popen(["pulseaudio", "-n", "--daemonize=no", "--exit-idle-time=-1", "--disable-shm=yes", "--use-pid-file=no", "--high-priority=no", "--realtime=no", "--log-level=warning", "-F", str(config)], env=env, stdout=daemon_log, stderr=daemon_log)
    try:
        deadline = time.monotonic() + 10
        while not (root / "native").exists():
            if daemon.poll() is not None or time.monotonic() >= deadline:
                raise RuntimeError(f"Private null server did not start; evidence: {root}")
            time.sleep(0.02)
        result = subprocess.run(["dbus-run-session", "--", str(binary), "--exact", "pulse_output::tests::native_routing_on_private_null_server", "--ignored", "--nocapture"], env=env, stdout=log, stderr=log, timeout=40)
    finally:
        daemon.terminate()
        daemon.wait(timeout=10)
print("Private null-Pulse evidence:", root)
raise SystemExit(result.returncode)
