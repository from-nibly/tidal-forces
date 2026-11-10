#!/usr/bin/env python3
"""Regenerate synthetic offline fixtures using the local OpenSSL CLI.

No network/device/account access. Private keys exist only in this script's private
TemporaryDirectory and are removed on exit. Only public certificates, signatures
and synthetic protocol records enter this directory. Regeneration changes keys
and the fixed fixture verification timestamp, so review the complete fixture diff.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

os.umask(0o077)
parser = argparse.ArgumentParser()
parser.add_argument('--out', type=Path, default=Path(__file__).resolve().parent)
parser.add_argument('--transport-keys', type=Path)
args = parser.parse_args()
OUT = args.out.resolve()
OUT.mkdir(parents=True, exist_ok=True)
if args.transport_keys:
    args.transport_keys = args.transport_keys.resolve()
    assert not args.transport_keys.is_relative_to(Path(__file__).resolve().parents[3])
    args.transport_keys.mkdir(mode=0o700, parents=True, exist_ok=False)


def varint(value):
    out = bytearray()
    while value > 127:
        out.append((value & 127) | 128)
        value >>= 7
    out.append(value)
    return bytes(out)


def integer(tag, value):
    return varint(tag << 3) + varint(value)


def blob(tag, value):
    return varint((tag << 3) | 2) + varint(len(value)) + value


with tempfile.TemporaryDirectory(prefix='tidal-cast-fixture-') as directory:
    root = Path(directory)

    def run(*args, data=None):
        return subprocess.run(['openssl', *args], cwd=root, input=data,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              check=True).stdout

    def key(name, bits=2048):
        run('genpkey', '-algorithm', 'RSA', '-pkeyopt', f'rsa_keygen_bits:{bits}',
            '-out', name + '.key')

    def cert(name, issuer=None, serial=1, ca=False, audio=False, days=365,
             digital=True, bits=2048, unknown_critical=False):
        key(name, bits)
        extensions = [
            'basicConstraints=critical,CA:TRUE,pathlen:2' if ca else 'basicConstraints=critical,CA:FALSE',
            'keyUsage=critical,keyCertSign,cRLSign' if ca else
            ('keyUsage=critical,digitalSignature' if digital else 'keyUsage=critical,keyEncipherment'),
            'subjectKeyIdentifier=hash',
        ]
        if not ca:
            extensions.append('extendedKeyUsage=clientAuth')
        if audio:
            extensions.append('certificatePolicies=1.3.6.1.4.1.11129.2.5.2')
        if unknown_critical:
            extensions.append('1.2.3.4.999=critical,DER:05:00')
        (root / (name + '.ext')).write_text('\n'.join(extensions) + '\n')
        subject = '/CN=Tidal synthetic fixture ' + name
        if issuer is None:
            args = ['req', '-new', '-x509', '-key', name + '.key', '-subj', subject,
                    '-days', str(days), '-sha256', '-set_serial', str(serial), '-out', name + '.pem']
            for extension in extensions:
                args += ['-addext', extension]
            run(*args)
        else:
            run('req', '-new', '-key', name + '.key', '-subj', subject, '-out', name + '.csr')
            run('x509', '-req', '-in', name + '.csr', '-CA', issuer + '.pem', '-CAkey',
                issuer + '.key', '-set_serial', str(serial), '-days', str(days),
                '-sha256', '-extfile', name + '.ext', '-out', name + '.pem')
        der = run('x509', '-in', name + '.pem', '-outform', 'DER')
        (OUT / (name + '.der')).write_bytes(der)
        return der

    def sign(name, data):
        return run('dgst', '-sha256', '-sign', name + '.key', data=data)

    def spki(name):
        public = run('x509', '-in', name + '.pem', '-pubkey', '-noout')
        return run('pkey', '-pubin', '-outform', 'DER', data=public)

    certificates = {}
    for name in ['root', 'crl-root', 'untrusted-root']:
        certificates[name] = cert(name, ca=True, days=3650)
    certificates['audio-root'] = cert('audio-root', ca=True, audio=True, days=3650)
    certificates['intermediate'] = cert('intermediate', 'root', serial=10, ca=True)
    certificates['audio-intermediate'] = cert('audio-intermediate', 'root', serial=11, ca=True, audio=True)
    for name, issuer, serial, audio, days, digital, bits in [
        ('device', 'intermediate', 42, False, 365, True, 2048),
        ('audio-device', 'intermediate', 43, True, 365, True, 2048),
        ('audio-chain-device', 'audio-intermediate', 44, False, 365, True, 2048),
        ('audio-root-device', 'audio-root', 45, False, 365, True, 2048),
        ('short-device', 'intermediate', 46, False, 1, True, 2048),
        ('wrong-usage-device', 'intermediate', 47, False, 365, False, 2048),
        ('weak-device', 'intermediate', 48, False, 365, True, 1024),
        ('crl-signer', 'crl-root', 50, False, 365, True, 2048),
        ('untrusted-crl-signer', 'untrusted-root', 51, False, 365, True, 2048),
    ]:
        certificates[name] = cert(name, issuer, serial, audio=audio, days=days,
                                  digital=digital, bits=bits)
    certificates['critical-device'] = cert('critical-device', 'intermediate', 52, unknown_critical=True)
    certificates['short-crl-signer'] = cert('short-crl-signer', 'crl-root', 53, days=1)
    # A legacy chain signature is a negative fixture, never an accepted policy.
    run('x509', '-req', '-in', 'device.csr', '-CA', 'intermediate.pem', '-CAkey',
        'intermediate.key', '-set_serial', '49', '-days', '365', '-sha1',
        '-extfile', 'device.ext', '-out', 'sha1-device.pem')
    certificates['sha1-device'] = run('x509', '-in', 'sha1-device.pem', '-outform', 'DER')
    (OUT / 'sha1-device.der').write_bytes(certificates['sha1-device'])
    certificates['tls'] = cert('tls', days=2)
    certificates['tls-other'] = cert('tls-other', days=2)
    certificates['tls-long'] = cert('tls-long', days=5)
    if args.transport_keys:
        for name in ['tls', 'device']:
            private = run('pkcs8', '-topk8', '-nocrypt', '-in', name + '.key', '-outform', 'DER')
            (args.transport_keys / (name + '.pk8')).write_bytes(private)
    now = int(time.time())
    nonce = bytes(range(16))

    def crl(name, revoked_keys=(), ranges=(), start=None, end=None, version=0,
            signer='crl-signer'):
        tbs = integer(1, version) + integer(2, now - 60 if start is None else start)
        tbs += integer(3, now + 3600 if end is None else end)
        for value in revoked_keys:
            tbs += blob(4, value)
        for issuer, first, last in ranges:
            tbs += blob(5, blob(1, issuer) + integer(2, first) + integer(3, last))
        signed = blob(1, tbs) + blob(2, certificates[signer]) + blob(3, sign(signer, tbs))
        result = blob(1, signed)
        (OUT / (name + '.crl')).write_bytes(result)
        return result

    clean = crl('clean')
    crl('long-list-short-signer', end=now + 3 * 86400, signer='short-crl-signer')
    crl('expired', end=now - 1)
    crl('future', start=now + 60)
    crl('unsupported-version', version=1)
    crl('untrusted-signer', signer='untrusted-crl-signer')
    crl('bad-hash-length', revoked_keys=[b'bad'])
    crl('too-many-keys', revoked_keys=[bytes(32)] * 257)
    for name in ['device', 'intermediate', 'root']:
        crl('revoked-' + name, revoked_keys=[hashlib.sha256(spki(name)).digest()])
    issuer_hash = hashlib.sha256(spki('intermediate')).digest()
    crl('revoked-serial', ranges=[(issuer_hash, 42, 42)])
    crl('serial-outside-range', ranges=[(issuer_hash, 43, 99)])
    crl('invalid-range', ranges=[(issuer_hash, 43, 42)])

    def reply(name, leaf, intermediates, tls='tls', signer=None):
        response = blob(1, sign(signer or leaf, nonce + certificates[tls])) + blob(2, certificates[leaf])
        for intermediate in intermediates:
            response += blob(3, certificates[intermediate])
        response += integer(4, 1) + blob(5, nonce) + integer(6, 1) + blob(7, clean)
        auth = blob(2, response)
        wire = integer(1, 0) + blob(2, b'receiver-0') + blob(3, b'sender-0')
        wire += blob(4, b'urn:x-cast:com.google.cast.tp.deviceauth') + integer(5, 1) + blob(7, auth)
        (OUT / (name + '.reply')).write_bytes(wire)

    for name in ['device', 'audio-device', 'short-device', 'wrong-usage-device', 'weak-device', 'critical-device']:
        reply(name, name, ['intermediate'])
    reply('audio-chain-device', 'audio-chain-device', ['audio-intermediate'])
    reply('audio-root-device', 'audio-root-device', [])
    reply('long-tls', 'device', ['intermediate'], 'tls-long')
    reply('sha1-device', 'sha1-device', ['intermediate'], signer='device')
    (OUT / 'clock.txt').write_text(str(now) + '\n')
    hashes = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
              for p in sorted(OUT.iterdir()) if p.suffix in ['.der', '.reply', '.crl', '.txt']}
    (OUT / 'SHA256.json').write_text(json.dumps(hashes, indent=2) + '\n')
print('Generated synthetic fixtures; generator temporary keys removed.' +
      (' Ephemeral transport keys belong to the private test runner.' if args.transport_keys else ' No private keys retained.'))
