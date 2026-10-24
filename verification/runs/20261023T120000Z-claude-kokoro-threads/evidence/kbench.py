# Back-to-back Kokoro synth timing through Atlas's own sherpa-onnx 1.13.8 C API.
# Usage: kbench.py <threads> <calls> <gap_s> <label>
import ctypes as C, os, sys, time, json
A = r'C:\Users\erics\AppData\Local\Atlas'
os.add_dll_directory(A + r'\tools\kokoro')
lib = C.CDLL(A + r'\tools\kokoro\sherpa-onnx-c-api.dll')
P, F, I = C.c_char_p, C.c_float, C.c_int32
def S(name, fields): return type(name, (C.Structure,), {'_fields_': fields})
Vits = S('Vits', [('model',P),('lexicon',P),('tokens',P),('data_dir',P),('noise_scale',F),('noise_scale_w',F),('length_scale',F),('dict_dir',P)])
Matcha = S('Matcha', [('acoustic_model',P),('vocoder',P),('lexicon',P),('tokens',P),('data_dir',P),('noise_scale',F),('length_scale',F),('dict_dir',P)])
Kok = S('Kok', [('model',P),('voices',P),('tokens',P),('data_dir',P),('length_scale',F),('dict_dir',P),('lexicon',P),('lang',P)])
Kit = S('Kit', [('model',P),('voices',P),('tokens',P),('data_dir',P),('length_scale',F)])
Zip = S('Zip', [('tokens',P),('encoder',P),('decoder',P),('vocoder',P),('data_dir',P),('lexicon',P),('feat_scale',F),('t_shift',F),('target_rms',F),('guidance_scale',F)])
Poc = S('Poc', [('lm_flow',P),('lm_main',P),('encoder',P),('decoder',P),('text_conditioner',P),('vocab_json',P),('token_scores_json',P),('cap',I)])
Sup = S('Sup', [('a',P),('b',P),('c',P),('d',P),('e',P),('f',P),('g',P)])
Model = S('Model', [('vits',Vits),('num_threads',I),('debug',I),('provider',P),('matcha',Matcha),('kokoro',Kok),('kitten',Kit),('zipvoice',Zip),('pocket',Poc),('supertonic',Sup)])
Tts = S('Tts', [('model',Model),('rule_fsts',P),('max_num_sentences',I),('rule_fars',P),('silence_scale',F)])
Audio = S('Audio', [('samples',C.POINTER(F)),('n',I),('sample_rate',I)])
lib.SherpaOnnxGetVersionStr.restype = P
assert lib.SherpaOnnxGetVersionStr() == b'1.13.8', lib.SherpaOnnxGetVersionStr()
lib.SherpaOnnxCreateOfflineTts.restype = C.c_void_p
lib.SherpaOnnxCreateOfflineTts.argtypes = [C.POINTER(Tts)]
lib.SherpaOnnxOfflineTtsGenerate.restype = C.POINTER(Audio)
lib.SherpaOnnxOfflineTtsGenerate.argtypes = [C.c_void_p, P, I, F]
lib.SherpaOnnxDestroyOfflineTtsGeneratedAudio.argtypes = [C.POINTER(Audio)]
lib.SherpaOnnxDestroyOfflineTts.argtypes = [C.c_void_p]
m = A + r'\models\kokoro'
b = lambda s: s.encode()
cfg = Tts()
k = cfg.model.kokoro
k.model, k.voices, k.tokens = b(m+r'\model.int8.onnx'), b(m+r'\voices.bin'), b(m+r'\tokens.txt')
k.data_dir, k.dict_dir = b(m+r'\espeak-ng-data'), b(m+r'\dict')
k.lexicon = b(m+r'\lexicon-us-en.txt,'+m+r'\lexicon-zh.txt'); k.length_scale = 1.0
threads, calls, gap, label = int(sys.argv[1]), int(sys.argv[2]), float(sys.argv[3]), sys.argv[4]
cfg.model.num_threads = threads; cfg.model.provider = b'cpu'
cfg.max_num_sentences = 1; cfg.silence_scale = 0.2
t0 = time.perf_counter(); tts = lib.SherpaOnnxCreateOfflineTts(C.byref(cfg)); load = time.perf_counter() - t0
assert tts, 'load failed'
text = b"The backup of your documents finished a minute ago, and everything checked out."
rows = []
for i in range(calls):
    t = time.perf_counter(); a = lib.SherpaOnnxOfflineTtsGenerate(tts, text, 3, 1.0); dt = time.perf_counter() - t
    secs = a.contents.n / a.contents.sample_rate; lib.SherpaOnnxDestroyOfflineTtsGeneratedAudio(a)
    rows.append(round(dt / secs, 2))
    if gap: time.sleep(gap)
lib.SherpaOnnxDestroyOfflineTts(tts)
print(json.dumps({'label': label, 'threads': threads, 'gap': gap, 'load_s': round(load, 2), 'speech_s': round(secs, 2), 'rtf_per_call': rows}))
