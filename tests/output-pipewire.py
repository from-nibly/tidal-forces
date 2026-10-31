#!/usr/bin/env python3
"""Non-audible PipeWire contract: private bus/runtime, null sinks, policy-only manager."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import threading
import struct

os.umask(0o077)
build = subprocess.run(["cargo", "test", "--locked", "--bin", "tidal-forces", "--no-run", "--message-format=json"], check=True, text=True, stdout=subprocess.PIPE)
executables = [m["executable"] for line in build.stdout.splitlines() if (m := json.loads(line)).get("reason") == "compiler-artifact" and m.get("profile", {}).get("test") and m.get("executable") and m["target"]["name"] == "tidal-forces"]
assert len(executables) == 1
root = Path(tempfile.mkdtemp(prefix="tidal-pipewire-test-", dir="/tmp"))
env = os.environ.copy()
for key in list(env):
    if key.startswith(("PIPEWIRE_", "PULSE_", "WIREPLUMBER_")) or key in ["DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET"]:
        env.pop(key, None)
for key in ["HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME"]:
    folder = root / key.lower()
    folder.mkdir(mode=0o700)
    env[key] = str(folder)
env.update(XDG_RUNTIME_DIR=str(root), PIPEWIRE_RUNTIME_DIR=str(root), PIPEWIRE_REMOTE="pipewire-0", PIPEWIRE_CONFIG_DIR=str(root), PULSE_RUNTIME_PATH=str(root), PULSE_SERVER=f"unix:{root}/native", DBUS_SESSION_BUS_ADDRESS=f"unix:path={root}/bus", DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={root}/no-system-bus", TIDAL_TEST_ISOLATED_PIPEWIRE=str(root), WIREPLUMBER_CONFIG_DIR=str(root / "wp"))
fixtures = Path(__file__).resolve().parent / "fixtures"
shutil.copyfile(fixtures / "pipewire-null.conf", root / "pipewire.conf")
shutil.copyfile(fixtures / "pipewire-pulse-null.conf", root / "pipewire-pulse.conf")
wp = Path(shutil.which("wireplumber")).resolve()
wp_config = wp.parent.parent / "share/wireplumber/wireplumber.conf"
if not wp_config.is_file():
    wp_config = Path("/usr/share/wireplumber/wireplumber.conf")
assert wp_config.is_file(), "WirePlumber 0.5 policy configuration required"
(root / "wp/wireplumber.conf.d").mkdir(parents=True)
shutil.copyfile(wp_config, root / "wp/wireplumber.conf")
shutil.copyfile(fixtures / "wireplumber-policy-only.conf", root / "wp/wireplumber.conf.d/99-policy.conf")
processes = []
logs = []
def start(name, args):
    log = (root / (name + ".log")).open("w")
    logs.append(log)
    process = subprocess.Popen(args, env=env, stdout=log, stderr=log)
    processes.append(process)
    return process

def wait_file(path):
    deadline = time.monotonic() + 8
    while not path.exists():
        assert all(p.poll() is None for p in processes) and time.monotonic() < deadline, f"Private daemon failed; evidence: {root}"
        time.sleep(0.05)

try:
    start("dbus", ["dbus-daemon", "--session", "--nofork", "--nopidfile", f"--address={env['DBUS_SESSION_BUS_ADDRESS']}"])
    wait_file(root / "bus")
    start("pipewire", ["pipewire", "-c", "pipewire.conf"])
    wait_file(root / "pipewire-0")
    start("policy", ["wireplumber", "--profile=policy"])
    start("pulse", ["pipewire-pulse", "-c", "pipewire-pulse.conf"])
    wait_file(root / "native")
    deadline = time.monotonic() + 8
    while True:
        sinks = subprocess.run(["pactl", "-f", "json", "list", "sinks"], env=env, text=True, capture_output=True, timeout=3)
        if sinks.returncode == 0:
            names = sorted(s["name"] for s in json.loads(sinks.stdout))
            if names == ["fixture_a", "fixture_b"]:
                break
            assert not names or all(n in ["fixture_a", "fixture_b"] for n in names), "Unexpected sink in private graph"
        assert all(p.poll() is None for p in processes) and time.monotonic() < deadline, f"Private null sinks not ready; evidence: {root}"
        time.sleep(0.1)
    deadline = time.monotonic() + 8
    while subprocess.run(["pactl", "set-default-sink", "fixture_a"], env=env, capture_output=True, timeout=3).returncode != 0:
        assert all(p.poll() is None for p in processes) and time.monotonic() < deadline, f"Private policy not ready; evidence: {root}"
        time.sleep(0.1)
    monitor_log = (root / "monitor.log").open("w")
    logs.append(monitor_log)
    monitor = subprocess.Popen(["parec", "--raw", "--format=float32le", "--rate=48000", "--channels=2", "--device=fixture_a.monitor"], env=env, stdout=subprocess.PIPE, stderr=monitor_log)
    processes.append(monitor)
    sample_counts = {"total": 0, "nonzero": 0}
    monitor_errors = []
    def count_samples():
        try:
            while chunk := monitor.stdout.read(4096):
                assert len(chunk) % 4 == 0
                samples = [v[0] for v in struct.iter_unpack("<f", chunk)]
                sample_counts["total"] += len(samples)
                sample_counts["nonzero"] += sum(abs(v) > 0.01 for v in samples)
        except Exception as error:
            monitor_errors.append(type(error).__name__)
    reader = threading.Thread(target=count_samples, daemon=True)
    reader.start()
    result = subprocess.run([executables[0], "--exact", "pulse_output::tests::song_changes_on_private_pipewire", "--ignored", "--nocapture"], env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=30)
    (root / "contract.log").write_text(result.stdout)
    print(result.stdout)
    monitor.terminate()
    monitor.wait(timeout=5)
    reader.join(timeout=5)
    assert not reader.is_alive()
    (root / "sample-counts.json").write_text(json.dumps(sample_counts))
    print("Synthetic null-monitor sample counts:", sample_counts)
    result.check_returncode()
    assert not monitor_errors, "Synthetic monitor reader failed"
    assert sample_counts["nonzero"] > 6 * 48000 * 2, "Native stream consumed audio but did not deliver sustained nonzero samples"
finally:
    for process in reversed(processes):
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
    for log in logs:
        log.close()
    print(f"Private PipeWire evidence: {root}")
