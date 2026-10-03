#!/bin/sh
# The wake-word clips for tests/the_name_and_the_request_in_one_breath.rs
# (29 Sep 2026): espeak-ng, one voice (en-us+m3, the corpus's speaker A),
# 16 kHz mono 16-bit, as Atlas records.
set -e
say() {
  espeak-ng -v en-us+m3 -s "$3" -p 45 -w "$1.raw.wav" "$2"
  ffmpeg -loglevel error -y -i "$1.raw.wav" -ar 16000 -ac 1 -sample_fmt s16 "$1.wav"
  rm "$1.raw.wav"
}
say atlas_what_time "Atlas, what time is it" 160
say atlas_alone "Atlas." 160
say what_time "What time is it?" 160
say atlas_long "Atlas, remind me to call the bank about the transfer tomorrow morning at nine" 150
