# Cast discovery documentation and implementation audit

## Scope and conclusion

Reviewed 2026-11-09 after the first development picker returned no devices.
This audit used public documents/source only; it did not perform receiver or
account-media contacts, further LAN probes, service changes or preview replacement.

**The protocol choice is correct, but the picker is not complete.** Google explicitly
identifies `_googlecast._tcp.local` as the service to inspect. Native DNS-SD/mDNS
browsing is appropriate; there is no evidence that discovery needs an account token,
a private Google enumeration API or an IP/subnet sweep. The existing zero-result
observations do not establish a root cause or prove a network configuration fault.

## Corrected implementation and retry — 2026-11-10

The operator explicitly authorized applying these findings and retrying discovery.
The native browser now streams coalesced, bounded updates until its owner closes
it; dropping the picker handle cancels browsing and shuts down the daemon. It
tracks add/update/remove events, validates and normalizes TXT Cast UUIDs, deduplicates
aliases, sanitizes friendly name/model, and preserves the resolved IPv4 SRV port.
Service associations are bounded to128 and displayed devices to64. Unsupported
addresses, malformed IDs and zero ports are rejected without logging peer values.
Transport/local-return checks now receive the selected advertised endpoint rather
than replacing its port with8009. This is endpoint handling, not a claim that audio
groups, IPv6 or every discovered receiver class has passed playback validation.

Private tests cover early streamed updates, coalescing, terminal delivery, cancelled
ownership, worker loss, identity/alias handling, rename/removal, bounds and advertised
ports. The running review preview and installed executable remain unchanged.

**The corrected 30-second live browse still found zero devices:** zero ServiceFound,
zero resolved, zero rejected. Browser shutdown was confirmed. Private receipt:
`/tmp/tidal-p7-discovery-stream-jv69lerf`. No receiver sessions, account lookups,
playback or device-control commands were performed.

A second independent30-second discovery-only browser using Python Zeroconf0.148.0
(the maintained implementation used by PyChromecast) on this same computer also
reported zero service events/devices, with no reported startup error. This used a
temporary Nix shell, not a daemon installation or profile/service change. No DIAL,
known-host polling, cached addresses or receiver sessions were used. Private receipt:
`/tmp/tidal-p7-discovery-zeroconf-7yx4ufxt`.

These results make a failure unique to our Rust inventory/filtering less likely;
they still do not prove a firewall/router fault or establish why advertisements
are absent. Discovery remains an unresolved product gate. Further host-side
multicast/interface/firewall-path investigation must be read-only unless separately
authorized; do not change system/router settings to manufacture a passing result.

## Home Manager repository / NixOS host investigation — 2026-11-10

The operator then requested investigating `~/.config/home-manager`. Its instructions
were read first; that repository was clean and was not edited. This repository
owns standalone Home Manager **and** separate NixOS system outputs; firewall policy
belongs to the latter, not a home activation.

The active device selects the Hyprland NixOS companion, which imports
`nixos/desktop.nix`. That module enables NetworkManager but declares no mDNS policy.
Evaluation of the active device's NixOS configuration returned:

- `networking.firewall.enable = true` (the upstream NixOS default).
- `allowedUDPPorts = []`, `allowedUDPPortRanges = []`, `allowedTCPPorts = []`.
- Only loopback is trusted; reverse-path filtering is enabled.
- Avahi, systemd-resolved, Tailscale and WireGuard are not enabled/configured.
- Avahi's `openFirewall = true` is inert because Avahi itself is disabled. The
  upstream module adds UDP5353 only inside its enabled configuration.

On the running host, `firewall.service` is active and NetworkManager is active.
The other checked discovery/resolver/VPN daemons are inactive. The deployed
firewall-start script has no allowed UDP port rules, accepts established/related
traffic and loopback, then drops remaining traffic, including multicast. This is
strong configuration evidence for a missing inbound mDNS allowance and is consistent
with both independent browsers seeing no advertisements. Checking only inactive
firewalld/ufw/nftables services would miss the NixOS-managed firewall; the installed
iptables tool itself uses the nf_tables backend.

Current kernel chain inspection as the unprivileged user was denied. No privilege
escalation, rule changes, rebuild/switch, daemon start, packet capture or additional
LAN probes were performed. Therefore this is not yet packet-level confirmation
that the missing allowance is the sole cause. A narrow inbound UDP5353 allowance
(preferably scoped to the intended LAN interface) in the device's **NixOS system**
configuration is the candidate correction; disabling the firewall, trusting the
whole interface or adding an Avahi daemon is not necessary for the native browser.
Any edit, temporary rule or system activation still needs explicit authorization,
followed by read-only rule verification and a bounded discovery recheck.

The preserved local device inventory checksum still matches the deployment guard.
Evaluation evidence: `/tmp/tidal-p7-network-config-eval.json`.

## Post-firewall discovery verification — 2026-11-10

The owning system session obtained separate operator approval, activated the narrow
WiFi-interface UDP5353 rule against the exact deployed baseline, and verified both
IPv4/IPv6 kernel rules. No Avahi daemon or global firewall opening was added. Its
activation receipt is `~/.local/state/gpd-mdns-firewall.LTKgHgou/ACTIVATED.md`.

The **same corrected native test binary** then passed its30-second discovery retry:
**five devices, six service-found/resolved events, one rejected record, five streamed
updates**. Browser shutdown was confirmed; the running preview's process identity
and executable digest were unchanged. No names/addresses were logged and no receiver
sessions, account lookups or playback were performed. Receipt:
`/tmp/tidal-p7-discovery-firewall-gmbodh2h/result.json`.

The narrow host firewall correction therefore resolved the observed discovery
failure. It does not establish playback compatibility for every discovered device,
explain the rejected record, or replace pending native picker/artwork/queue review.

## Primary sources

- Google, [Discovery Troubleshooting](https://developers.google.com/cast/docs/discovery):
  check the same sender/network; inspect `_googlecast._tcp.local`; Linux example
  uses Avahi. Google specifically excludes Netflix/YouTube as comparison tests
  because they use specialized discovery. Its troubleshooting suggestions are
  diagnostic options, not authorization to restart devices or change the network.
- Google, [Android sender integration — Configure device discovery](https://developers.google.com/cast/docs/android_sender_integrate):
  the SDK manages discovery and runs it while the Cast dialog is open; dialog close
  or backgrounding stops discovery. SDK session recovery is not adopted by this
  player, whose no-automatic-reconnect rule remains unchanged.
- Google, [Cast button design checklist](https://developers.google.com/cast/docs/design_checklist/cast-button):
  consistent access from playable screens; disconnected, connecting and connected
  states, with progressive waves during connection and a filled frame when connected.
- Google, [Cast dialog design checklist](https://developers.google.com/cast/docs/design_checklist/cast-dialog):
  device selection and connected-session controls belong behind the Cast button.
- [RFC6762 §5.2](https://www.rfc-editor.org/rfc/rfc6762#section-5.2):
  service-browser results are a continuously maintained set for the duration of
  the browsing window, not a fixed one-shot snapshot. Retransmissions require
  backoff and known-answer suppression; delegate these to the mDNS implementation.
  IPv4 uses224.0.0.251:5353; IPv6 uses[ff02::fb]:5353. These are public protocol
  multicast constants, not private receiver addresses.
- [RFC6763 §§4–5](https://www.rfc-editor.org/rfc/rfc6763#section-5):
  browse PTR records, resolve instance SRV/TXT, then target A/AAAA. The SRV record
  specifies the endpoint port; clients must not replace it with a guessed constant.

## Established implementations checked

### Chromium

[cast_media_sink_service.cc](https://github.com/chromium/chromium/blob/main/chrome/browser/media/router/discovery/mdns/cast_media_sink_service.cc)
starts a DNS-SD listener, optionally after user interaction. Service events update
its available sink set. [service_discovery_device_lister.cc](https://github.com/chromium/chromium/blob/main/chrome/browser/local_discovery/service_discovery_device_lister.cc)
keeps service watchers, resolves added/updated services, removes disappeared entries
and handles permission rejection/cache invalidation.

[media_sink_util.cc](https://github.com/chromium/chromium/blob/main/chrome/browser/media/router/discovery/mdns/media_sink_util.cc)
uses TXT `id` as device identity, `fn` as friendly name, `md` as model, and `ca` as
capabilities; it takes the port from the resolved service. It does not require
all services to advertise8009. [cast_media_sink_service_impl.cc](https://github.com/chromium/chromium/blob/main/chrome/browser/media/router/discovery/mdns/cast_media_sink_service_impl.cc)
also has DIAL-discovered and cached-network paths. Therefore a working Chrome list
would not, by itself, prove that a fresh generic mDNS browse succeeded.

Chromium proactively opens channels to candidate sinks; this player must **not**
copy that behavior without separate scope. Advertisements remain untrusted, and
receiver authentication/control is restricted to the selected device.

### PyChromecast

[discovery.py](https://github.com/home-assistant-libs/pychromecast/blob/master/pychromecast/discovery.py)
uses a persistent Zeroconf ServiceBrowser, resolves service information and tracks
add/update/remove callbacks. It parses TXT `id` as UUID and deduplicates by that
identity, not friendly name or service-instance name. It reads name/model/capabilities,
uses the advertised service port and ignores subtype service entries as devices.
Its host browser can retain known hosts when mDNS fails; that is not evidence that
ordinary mDNS succeeded, and is not a reason to hide manual cached addresses in
our new-device picker. [dial.py](https://github.com/home-assistant-libs/pychromecast/blob/master/pychromecast/dial.py)
selects resolved addresses, preferring IPv4 but supporting scoped IPv6.

### mdns-sd0.21.7

The downloaded crate API/source does implement the expected multicast memberships,
port5353, per-interface handling, IPv4/IPv6, service resolution and add/remove events.
There is no evidence from this review that simply replacing the library would fix
our observed failure. The application's lifecycle/model around it needs correction.
The library's browser can live until cancellation without hand-written constant
polling. Network/interface and packet behavior still needs independent validation.

## Application gaps identified before the correction

1. `src/cast_discovery.rs` previously returned one final snapshot after eight seconds.
   Stream bounded updates while the picker is open; cancel and shut down the browser
   when it closes. Show devices as soon as resolved, and process changes/removals.
   A short startup hint may expire; the browser itself should not silently stop.
2. Previous device identity was the DNS service full name. Parse/validate TXT `id`,
   deduplicate by Cast identity, retain service associations, and treat every value
   as untrusted metadata rather than authentication. Preserve sanitized name/model
   and capability information for presentation and supported-device gating.
3. The previous filter and runtime assumed port8009. Preserve the advertised endpoint;
   support only endpoint/device classes whose authenticated playback contracts are
   validated. Audio groups, IPv6 and unusual endpoint ports are not automatically
   proven by success on a single IPv4 receiver.
4. Distinguish browser startup failure, local permission/socket error, unresolved
   advertisements, and no observed services. Do not turn absence into a definitive
   firewall/router diagnosis. Bound/redact diagnostics; do not log peer addresses.
5. The player icon currently changes accent color only. Add proper connection states
   and consistent access across normal/mini/classic playback surfaces, without
   exposing IP entry or certificate-policy jargon.

## What the current failures do and do not show

The native browse observed zero ServiceFound and zero ServiceResolved events.
Separate raw multicast/ephemeral-port/interface-specific diagnostics also observed
zero response datagrams. The sender has an active WiFi interface and the selected
receiver record lies in a directly attached subnet. Those observations narrow the
investigation but neither prove correct transmitted packets nor explain why replies
were absent. The phone's successful discovery is a different sender path; the exact
comparison app was not identified, so specialized/cached discovery cannot be excluded.

Next verification should compare a maintained service browser **on this computer**
and exercise discovery against controlled service records before attributing the
result to the LAN. Verify complete PTR→SRV/TXT→A/AAAA resolution, add/update/remove,
interface selection and cancellation. Any subsequent live browse needs current
scope; do not restart services, change firewall/router settings, install a daemon,
contact all advertised receivers or restore manual-IP UX as a workaround.

Public source snapshots and extracted official documentation are retained locally in
`/tmp/tidal-p7-discovery-doc-audit/`. Branch-head source links above are mutable;
this document records the behavior inspected on the review date, not a release pin.
