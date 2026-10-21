param([string]$Exe, [string]$Root, [int]$RunSeconds = 150)
$ErrorActionPreference = 'Stop'
$home_ = Join-Path $Root ("probe-" + [guid]::NewGuid().ToString('N').Substring(0,8))
New-Item -ItemType Directory -Path $home_ | Out-Null
$env:ATLAS_HOME = $home_
# First start writes the shipped config; stop it straight away.
& $Exe hub address | Out-Null
$tools = Join-Path $home_ 'config\tools.yaml'
$t = Get-Content -Raw $tools
$port = Get-Random -Minimum 20000 -Maximum 45000
$t = [regex]::Replace($t, '(?m)(^server:\s*\r?\n\s+enabled:\s+true\s*\r?\n\s+port:\s*)8787', "`${1}$port")
$t = [regex]::Replace($t, '(?m)(^kin:\s*\r?\n\s+enabled:\s+false\s*\r?\n\s+port:\s*)8788', "`${1}$($port+1)")
Set-Content -LiteralPath $tools -Value $t -Encoding UTF8
$state = Join-Path $home_ 'data\state'
New-Item -ItemType Directory -Force -Path $state | Out-Null
Copy-Item 'C:\Users\erics\AppData\Local\Atlas\data\state\index.json' (Join-Path $state 'index.json')
$sizeBefore = (Get-Item (Join-Path $state 'index.json')).Length
$results = [ordered]@{ home = $home_; index_bytes_before = $sizeBefore }
foreach ($round in 1..2) {
  $sw = [Diagnostics.Stopwatch]::StartNew()
  $p = Start-Process -FilePath $Exe -ArgumentList @('--daemon') -PassThru -WindowStyle Hidden
  $hubReady = $null
  while ($sw.Elapsed.TotalSeconds -lt 60 -and $null -eq $hubReady) {
    try { $r = Invoke-WebRequest -UseBasicParsing -TimeoutSec 1 "http://127.0.0.1:$port/hub/ping"; if ($r.StatusCode -eq 200) { $hubReady = [math]::Round($sw.Elapsed.TotalSeconds,1) } } catch { Start-Sleep -Milliseconds 250 }
  }
  $results["round${round}_hub_ready_s"] = $hubReady
  $wait = if ($round -eq 1) { $RunSeconds } else { 15 }
  Start-Sleep -Seconds $wait
  $p.Refresh(); $peak = if (-not $p.HasExited) { [math]::Round($p.WorkingSet64/1MB) } else { -1 }
  $results["round${round}_working_set_mb"] = $peak
  Set-Content -LiteralPath (Join-Path $state 'please_stop') -Value 'stop' -NoNewline
  $stopSw = [Diagnostics.Stopwatch]::StartNew()
  $p.WaitForExit(60000) | Out-Null
  $results["round${round}_stop_s"] = [math]::Round($stopSw.Elapsed.TotalSeconds,1)
  $results["round${round}_exited"] = $p.HasExited
  Start-Sleep -Seconds 2
}
$results.index_bytes_after = (Get-Item (Join-Path $state 'index.json')).Length
$results.set_aside_files = @(Get-ChildItem $state -Filter 'index.*.bak' | ForEach-Object Name)
$log = Join-Path $home_ 'data\logs\atlas.log'
$text = if (Test-Path $log) { Get-Content -Raw $log } else { '' }
$results.log_fetching_lines = ([regex]::Matches($text, 'fetching \d+ missing piece')).Count
$results.log_camera_frames = ([regex]::Matches($text, 'camera: one frame taken')).Count
$results.log_put_aside = ([regex]::Matches($text, 'put the file aside')).Count
$results.log_tray_lines = @([regex]::Matches($text, '(?m)^.*(tray|icon by the clock).*$') | ForEach-Object { $_.Value.Substring(0, [Math]::Min(160, $_.Value.Length)) })
$results.log_stopped_cleanly = ([regex]::Matches($text, 'stopped cleanly')).Count
$backlog = Join-Path $state 'backlog.json'
$results.download_ask_filed = (Test-Path $backlog) -and ((Get-Content -Raw $backlog) -match 'missing pieces')
$results.downloads_dir = @(Get-ChildItem -Recurse -ErrorAction SilentlyContinue (Join-Path $home_ 'data\tmp') | Measure-Object).Count
$results | ConvertTo-Json -Depth 4
