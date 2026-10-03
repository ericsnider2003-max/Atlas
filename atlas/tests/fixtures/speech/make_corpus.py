# Synthesized speakers for the round-5 voice work: six espeak-ng voices,
# each saying different sentences at a varied pitch and speed, so the same
# "person" is never twice alike. Also wake-word takes: "hey atlas" and
# near-misses. All 16 kHz mono 16-bit.
import subprocess, os, random, json, wave, struct
random.seed(5)
OUT = "corpus"
SPEAKERS = {"A": "en-us+m3", "B": "en-gb+f3", "C": "en-gb-scotland+m1",
            "D": "en-029+f2", "E": "en-gb-x-rp+m7", "F": "en-us+f4"}
SENT = ["the installer is coming on tuesday at ten",
        "check the server before the market opens",
        "send the invoice to the client this afternoon",
        "what is on my calendar for tomorrow morning",
        "remind me to call the bank about the transfer",
        "the euro dollar moved forty pips overnight",
        "open the budget file and the notes from friday",
        "we need a permit before the work can start",
        "read me the last three messages from dana",
        "turn the lights down and start the music"]
WAKE = ["hey atlas"] * 9
NEAR = ["hey alice", "that's a lot", "hey at last", "atlantic", "hey there", "at least"]
def say(text, voice, path):
    p = random.randint(35, 65); s = random.randint(145, 185)
    raw = path + ".raw.wav"
    subprocess.run(["espeak-ng", "-v", voice, "-p", str(p), "-s", str(s), "-w", raw, text], check=True)
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-i", raw, "-ar", "16000", "-ac", "1", "-sample_fmt", "s16", path], check=True)
    os.remove(raw)
os.makedirs(OUT, exist_ok=True)
index = []
for spk, voice in SPEAKERS.items():
    for i, t in enumerate(SENT):
        f = f"{OUT}/{spk}_s{i}.wav"; say(t, voice, f); index.append({"file": f, "speaker": spk, "kind": "sentence", "text": t})
for spk in ["A", "B"]:
    for i, t in enumerate(WAKE):
        f = f"{OUT}/{spk}_wake{i}.wav"; say(t, SPEAKERS[spk], f); index.append({"file": f, "speaker": spk, "kind": "wake", "text": t})
    for i, t in enumerate(NEAR):
        f = f"{OUT}/{spk}_near{i}.wav"; say(t, SPEAKERS[spk], f); index.append({"file": f, "speaker": spk, "kind": "near", "text": t})
json.dump(index, open(f"{OUT}/index.json", "w"), indent=0)
print(len(index), "files")
