$ErrorActionPreference = 'Stop'
$server = 'C:\Users\erics\AppData\Local\Atlas\tools\llama\llama-server.exe'
$model = 'C:\Users\erics\AppData\Local\Atlas\models\Qwen3-1.7B-Q4_K_M.gguf'
$dir = Join-Path $PSScriptRoot 'runs\20261107T-live-research-budget'
New-Item -ItemType Directory -Force $dir | Out-Null
    $p = Start-Process $server -ArgumentList @('-m',$model,'--host','127.0.0.1','--port','18080','-c','4096','-n','128','-ngl','0','-t','8','--parallel','1','--sleep-idle-seconds','-1','--no-webui') -RedirectStandardOutput (Join-Path $dir 'server.out.log') -RedirectStandardError (Join-Path $dir 'server.err.log') -WindowStyle Hidden -PassThru
try {
    $ready = $false
    for ($i = 0; $i -lt 90; $i++) {
        try { if ((Invoke-RestMethod 'http://127.0.0.1:18080/health' -TimeoutSec 2).status -eq 'ok') { $ready = $true; break } } catch { }
        Start-Sleep -Seconds 2
    }
    if (-not $ready) { throw 'server not ready' }
    $mingw = 'C:\Users\erics\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.MCF.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64'
    $env:PATH = "$mingw\bin;$env:PATH"
    $env:LIBRARY_PATH = "$mingw\x86_64-w64-mingw32\lib;$mingw\lib\gcc\x86_64-w64-mingw32\16.1.0"
    $env:RUST_MIN_STACK = '67108864'
    # Each live model run gets its own Cargo target directory. This prevents
    # concurrent pressure/review runs from sharing the default target lock or
    # partially-built artifacts.
    $env:CARGO_TARGET_DIR = Join-Path $dir 'target'
    $env:ATLAS_REAL_MODEL_URL = 'http://127.0.0.1:18080/v1/chat/completions'
    $env:ATLAS_LIVE_RESEARCH = '1'
    cargo +1.98.1-x86_64-pc-windows-gnu test --test a_real_model_answers_him his_sentences_against_a_real_model -- --nocapture
    $env:ATLAS_LIVE_MEMORY = '1'
    cargo +1.98.1-x86_64-pc-windows-gnu test --test a_real_model_answers_him live_local_model_memory_survives_restart -- --nocapture
    $env:ATLAS_LIVE_COUNCIL = '1'
    cargo +1.98.1-x86_64-pc-windows-gnu test --test a_real_model_answers_him live_local_model_can_run_atlas_council -- --nocapture
}
finally {
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    'SERVER_STOPPED'
}
