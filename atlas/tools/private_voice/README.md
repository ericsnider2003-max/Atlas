# Private voice provider

This directory contains only the public adapter contract. It must not contain
Eric's recordings or model weights. Configure Atlas's existing `chatterbox`
engine to point at a user-local copy of `speak.cmd`, with `voices_dir` set to
the private voice root.

The adapter receives text on stdin and the existing Atlas arguments:
`--reference <wav> --out <wav> --speed <value> --exaggeration <value>
--sentence-gap <value>`.

The user-local adapter may dispatch those arguments to F5-TTS, Chatterbox, or
another private runtime. It must write a mono 24 kHz WAV and return nonzero on
failure. Public Atlas packages include this contract only, never the private
runtime or model.
