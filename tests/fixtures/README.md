# Synthetic audio fixtures

`aac-lc/`, `he-aac/` and `he-aac-v2/` contain five-second, stereo 440 Hz tones
encoded using the bundled FDK encoder, then remuxed without re-encoding into DASH
fragments. Tests compare segmented playback sample-for-sample with a continuous
decode and validate seeking with codec preroll. No TIDAL music is included.

Regenerate these AAC fixtures (Bash):

```bash
cargo run --example make_aac_fixtures -- /tmp/aac-fixtures
for codec in aac-lc he-aac he-aac-v2; do
  mkdir -p "/tmp/aac-fixtures/$codec"
  ffmpeg -i "/tmp/aac-fixtures/$codec.aac" -c:a copy -f dash -seg_duration 1 \
    -use_timeline 1 -use_template 1 -init_seg_name '0.mp4' \
    -media_seg_name '$Number$.mp4' "/tmp/aac-fixtures/$codec/audio.mpd"
  cp /tmp/aac-fixtures/"$codec"/*.mp4 "tests/fixtures/$codec/"
done
```

The generated MP4s use an AAC-LC AudioSpecificConfig with implicit SBR/PS signaling
for HE-AAC variants. The live TIDAL fallback path is also checked with explicit
`mp4a.40.5` manifests. ffmpeg is only a fixture-generation tool, not a build,
test or player runtime dependency.

`dash/0.mp4` is a FLAC initialization fragment. `dash/1.mp4` through `4.mp4`
contain a synthesized 3.2-second tone split across four media fragments. The
DASH tests decode every frame and compare post-seek samples with the original
decoded stream, including seeks into fragments evicted from the cache.

Generated with ffmpeg (rename the initialization/media files to 0.mp4–4.mp4):

```bash
ffmpeg -f lavfi -i sine=frequency=440:sample_rate=44100 -t 3.2 \
  -c:a flac -strict -2 -f dash -seg_duration 1 -use_timeline 1 -use_template 1 audio.mpd
```

These are one-second 440 Hz tones generated locally, not downloaded music.
The AAC MP4 intentionally has its `moov` metadata at the end to exercise seeking.
Tests use the app's real HTTP streaming and native decoder path; no ffmpeg is
needed to run the tests or the application.

Regenerate (Bash, with ffmpeg installed):

```bash
ffmpeg -f lavfi -i sine=frequency=440:sample_rate=44100 -t 1 -c:a aac -b:a 64k tone.m4a
ffmpeg -f lavfi -i sine=frequency=440:sample_rate=44100 -t 1 -c:a flac tone.flac
```
