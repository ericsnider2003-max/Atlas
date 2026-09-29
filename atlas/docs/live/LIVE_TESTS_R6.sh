#!/bin/sh
# Live runs for round 6 (the rendering arms and the second look at who's who),
# through the real binary, the round-6 end-to-end tests and the voice
# measurements.   ./LIVE_TESTS_R6.sh <atlas crate>
set -u
A="$(cd "${1:-../atlas}" && pwd)"
HOME_DIR="$(mktemp -d)"; export ATLAS_HOME="$HOME_DIR"
CONF="$HOME_DIR/config"; cp -r "$A/config" "$CONF"; export ATLAS_CONFIG="$CONF"
ATLAS="$A/target/debug/atlas"; C="$A/tests/fixtures/speech/corpus"
# This container's Chromium isn't on the PATH; on Windows Atlas finds Edge itself.
[ -e /opt/pw-browsers/chromium ] && { mkdir -p "$HOME_DIR/bin"; ln -s /opt/pw-browsers/chromium "$HOME_DIR/bin/chromium"; PATH="$HOME_DIR/bin:$PATH"; }
run() { echo; echo "\$ $*" | sed "s#$HOME_DIR#<home>#g; s#$A/##g"; "$@" 2>&1 | grep -v "^note: not on Windows" | sed "s#$HOME_DIR#<home>#g"; }

echo "================ PERSONAL ATLAS, round 6 ================"

echo; echo "---- an SVG animation played and saved as a GIF and an MP4 ----"
cat > "$HOME_DIR/ball.svg" <<'SVG'
<svg xmlns="http://www.w3.org/2000/svg" width="240" height="120"><title>a ball rolling right</title>
<rect width="240" height="120" fill="#f7f4ee"/>
<circle cx="20" cy="70" r="14" fill="#d9730d"><animate attributeName="cx" from="20" to="220" dur="2s" repeatCount="indefinite"/></circle>
</svg>
SVG
run "$ATLAS" film "$HOME_DIR/ball.svg" 12
run ffprobe -v error -count_frames -select_streams v:0 -show_entries stream=codec_name,width,height,nb_read_frames -of csv=p=0 "$HOME_DIR/ball.gif"
run ffprobe -v error -count_frames -select_streams v:0 -show_entries stream=codec_name,width,height,nb_read_frames -of csv=p=0 "$HOME_DIR/ball.mp4"

echo; echo "---- a 3-D scene drawn in house: a still and a turntable ----"
cat > "$HOME_DIR/desk.json" <<'JSON'
{"width":320,"height":200,"camera":{"from":[3.2,2.4,5.0],"at":[0,0.7,0],"fov":40},"sun":[-0.6,1.0,0.7],
 "objects":[{"shape":"ground","y":0,"colour":"#d8d2c4"},
            {"shape":"box","at":[0,0.4,0],"size":[1.6,0.8,1.0],"colour":"#3a6fc4"},
            {"shape":"sphere","at":[0,1.25,0],"radius":0.45,"colour":"#d9730d","shine":0.4},
            {"shape":"cylinder","at":[1.4,0,0.6],"radius":0.25,"height":1.1,"colour":"#4f9a55"}]}
JSON
run "$ATLAS" scene "$HOME_DIR/desk.json"
run ffprobe -v error -count_frames -select_streams v:0 -show_entries stream=width,height,nb_read_frames -of csv=p=0 "$HOME_DIR/desk.turntable.gif"

echo; echo "---- who said what on a call: grouping alone, then with --merge-voices ----"
python3 - "$HOME_DIR/others.wav" "$HOME_DIR/call.wav" "$C" <<'PY'
import sys, wave, struct
def rd(p):
    w = wave.open(p); d = w.readframes(w.getnframes()); return list(struct.unpack('<%dh' % (len(d)//2), d))
def wr(p, s):
    w = wave.open(p, 'wb'); w.setnchannels(1); w.setsampwidth(2); w.setframerate(16000)
    w.writeframes(struct.pack('<%dh' % len(s), *[max(-32768, min(32767, x)) for x in s]))
C = sys.argv[3]; gap = [0]*12800
others = []
for spk in "DEF":
    for i in range(10): others += rd(f"{C}/{spk}_s{i}.wav") + gap
for i in range(6, 10): others += rd(f"{C}/A_s{i}.wav") + gap
wr(sys.argv[1], others)
call = []
for spk, i in [("A",0),("B",1),("A",2),("C",3),("B",4),("A",5)]: call += gap + rd(f"{C}/{spk}_s{i}.wav")
wr(sys.argv[2], call + gap)
PY
run "$ATLAS" voices "$HOME_DIR/others.wav"
run "$ATLAS" notes "$HOME_DIR/call.wav"
run "$ATLAS" notes "$HOME_DIR/call.wav" --merge-voices
echo "(truth: A, B, A, C, B, A)"

echo; echo "---- the round-6 end-to-end tests, with what each saw ----"
( cd "$A" && cargo test -q --test all round6 -- --nocapture --test-threads=1 2>&1 | grep -E "LIVE|test result" )

echo; echo "---- the voice measurements, including the second look on 60 calls ----"
( cd "$A" && cargo test -q --release --test voice_measured -- --nocapture --test-threads=1 2>&1 | grep -vE "^running|^$" )
rm -rf "$HOME_DIR"
