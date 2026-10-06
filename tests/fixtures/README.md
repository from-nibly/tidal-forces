# Synthetic audio fixtures

These are one-second 440 Hz tones generated locally, not downloaded music.
The AAC MP4 intentionally has its `moov` metadata at the end to exercise seeking.
Tests use the app's real HTTP streaming and native decoder path; no ffmpeg is
needed to run the tests or the application.

Regenerate (Bash, with ffmpeg installed):

```bash
ffmpeg -f lavfi -i sine=frequency=440:sample_rate=44100 -t 1 -c:a aac -b:a 64k tone.m4a
ffmpeg -f lavfi -i sine=frequency=440:sample_rate=44100 -t 1 -c:a flac tone.flac
```
