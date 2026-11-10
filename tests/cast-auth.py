#!/usr/bin/env python3
"""Private loopback TLS/auth contract. Never reads saved targets or contacts LAN devices."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

os.umask(0o077)
repo = Path(__file__).resolve().parents[1]
# Cargo only sees the normal build environment, not an altered HOME/keyring/profile.
# Set SSLKEYLOGFILE only on the test executable, not Cargo's own TLS/network stack.
build_env = os.environ.copy()
build_env.pop('SSLKEYLOGFILE', None)
build = subprocess.run(['cargo', 'test', '--locked', '--example', 'cast_auth_probe',
                        '--no-run', '--message-format=json'], cwd=repo, env=build_env,
                       stdout=subprocess.PIPE, text=True, check=True)
artifacts = [json.loads(line) for line in build.stdout.splitlines() if line.startswith('{')]
executables = [item['executable'] for item in artifacts
               if item.get('reason') == 'compiler-artifact' and item.get('executable')
               and item['target']['name'] == 'cast_auth_probe' and item['profile']['test']]
assert len(executables) == 1
with tempfile.TemporaryDirectory(prefix='tidal-cast-tls-') as directory:
    root = Path(directory)
    subprocess.run(['python3', str(repo / 'tests/fixtures/cast-auth/generate.py'),
                    '--out', str(root / 'fixtures'), '--transport-keys', str(root / 'keys')], env=build_env, check=True)
    env = build_env.copy()
    env['TIDAL_CAST_PRIVATE_FIXTURES'] = str(root)
    env['SSLKEYLOGFILE'] = str(root / 'forbidden-keylog')
    # The transport matrix includes multiple full-completion polling scenarios.
    # This is only the aggregate private-runner budget, not a live request timeout.
    for test in ['auth::transport::tests::private_tls_contract',
                 'auth::legacy_tests::private_legacy_contract']:
        subprocess.run([executables[0], test, '--ignored', '--exact'],
                       cwd=repo, env=env, check=True, timeout=90)
    assert not (root / 'forbidden-keylog').exists(), 'TLS key logging must remain disabled'
print('Private loopback TLS/auth contract passed; ephemeral fixture keys removed.')
