# Local processing contract

Processing runs on the bounded decoder worker before PCM publication, metering and
visualization. The output callback does not calculate coefficients, lock settings,
read media, or allocate processing buffers. Live changes can take up to the bounded
PCM lead to become audible. State resets at a seek/new decoder session.

All processing defaults off. Disabled and flat processing preserve finite sample
bits, including negative zero. Enabled processing supports ten fixed peaking bands,
preamp, stereo-only balance/mono, and normalization. Unsupported above-Nyquist bands
are not shifted to another frequency. Positive EQ bands reserve conservative summed
headroom; the explicitly processed path hard-limits peaks. Bypass does not clamp.
There is no track crossfade or remote-device DSP claim. Live parameter changes
blend processing states over five milliseconds on the worker; this does not overlap
songs. Start/resume, pause and volume use complete-frame five-millisecond output
envelopes. Once a pause reaches silence, source consumption stops, so those silent
frames do not advance queue progress or listening credit. Safety cancellation is
immediate, and seek/new loads start with a fresh envelope.

## Normalization evidence

TIDAL's official web SDK, revision
`c00f62cf4e0d4ece0b07f310a0b1fa1831193095`, implements
`min(10 ^ ((4 + replayGain) / 20), 1)` in
[`normalize-volume.ts`](https://github.com/tidal-music/tidal-sdk-web/blob/c00f62cf4e0d4ece0b07f310a0b1fa1831193095/packages/player/src/internal/helpers/normalize-volume.ts).
Its playback-info resolver identifies `trackReplayGain` and `albumReplayGain`.
This establishes a dB gain input, +4 dB preamp and an attenuation-only policy.

The implementation follows that explicit policy, using only finite supplied gain
values in the accepted range. Missing/invalid values stay unavailable, and album
mode never substitutes track data. The SDK's unity ceiling is a processing policy,
not fabricated peak metadata. Provider `trackPeakAmplitude`/`albumPeakAmplitude`
fields are deliberately not interpreted; no verified peak value is invented or
claimed. Synthetic peak tests cover the local processing limiter independently.

Continuous matching-FLAC boundaries remain eligible with DSP bypass or stateless
processing with matching normalization metadata. Stateful EQ and changing gain
metadata use ordinary transitions rather than claiming unverified filter-state
continuity. AAC priming/padding and mixed-format boundaries retain their existing
capability gates. No envelope is restarted inside an eligible continuous handoff.

The combined v0.31 native preview is approved. Real hardware validation remains
separately opt-in. These software contracts are not a claim of measured
hardware/DAC behavior.
