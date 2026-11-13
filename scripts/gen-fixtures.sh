#!/usr/bin/env bash
# Regenerate the tiny audio fixtures used by the test-suite.
# Each clip is a 1-second 1 kHz sine tone tagged with known metadata.
set -euo pipefail

cd "$(dirname "$0")/../tests/fixtures"

pre=(-hide_banner -loglevel error -y)
tags=(-fflags +bitexact -map_metadata -1
      -metadata "title=Test Tone" -metadata "artist=eleven" -metadata "album=Fixtures")
stereo=(-f lavfi -i "sine=frequency=1000:sample_rate=44100:duration=1" "${tags[@]}" -ac 2)

# 22.05 kHz mono WAV: exercises mono->stereo mapping and resampling.
ffmpeg "${pre[@]}" -f lavfi -i "sine=frequency=1000:sample_rate=22050:duration=1" \
  "${tags[@]}" -ac 1 -c:a pcm_s16le -flags:a +bitexact tone.wav
ffmpeg "${pre[@]}" "${stereo[@]}" -c:a flac -flags:a +bitexact tone.flac
ffmpeg "${pre[@]}" "${stereo[@]}" -c:a libmp3lame -b:a 128k -flags:a +bitexact tone.mp3
ffmpeg "${pre[@]}" "${stereo[@]}" -c:a libvorbis -q:a 2 -flags:a +bitexact tone.ogg
ffmpeg "${pre[@]}" "${stereo[@]}" -c:a aac -b:a 96k -flags:a +bitexact tone.m4a
# A chained Ogg stream (as sent by Ogg radio stations): the 44.1 kHz clip followed by a
# 22.05 kHz one with its own serial number, to test sample-rate changes mid-stream.
ffmpeg "${pre[@]}" -f lavfi -i "sine=frequency=1000:sample_rate=22050:duration=1" \
  "${tags[@]}" -ac 2 -c:a libvorbis -q:a 2 -flags:a +bitexact -serial_offset 1 chained-tail.ogg
cat tone.ogg chained-tail.ogg > chained.ogg
rm chained-tail.ogg
# A file with a valid extension but garbage content, for error-path tests.
printf 'this is not audio data, it just goes to eleven\n%.0s' {1..64} > corrupt.mp3

ls -l
