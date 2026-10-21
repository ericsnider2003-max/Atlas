$ErrorActionPreference = 'Stop'
$A = 'C:\Users\erics\AppData\Local\Atlas'
$state = "$A\data\state"
$log = "$A\data\logs\atlas.log"
$logStart = (Get-Item $log).Length
$out = [ordered]@{ index_bytes_before = (Get-Item "$state\index.json").Length }
$sw = [Diagnostics.Stopwatch]::StartNew()
$p = Start-Process -FilePath "$A\atlas.exe" -ArgumentList @('--daemon') -PassThru -WindowStyle Hidden
$ready = $null
while ($sw.Elapsed.TotalSeconds -lt 60 -and $null -eq $ready) {
  foreach ($port in 8787, 8789) {
    try { $r = Invoke-WebRequest -UseBasicParsing -TimeoutSec 1 "http://127.0.0.1:$port/hub/ping"; if ($r.StatusCode -eq 200) { $ready = [math]::Round($sw.Elapsed.TotalSeconds,1); $out.port = $port; break } } catch {}
  }
  if ($null -eq $ready) { Start-Sleep -Milliseconds 250 }
}
$out.hub_ready_s = $ready
Start-Sleep -Seconds 150
$p.Refresh()
$out.working_set_mb = [math]::Round($p.WorkingSet64/1MB)
$out.private_mb = [math]::Round($p.PrivateMemorySize64/1MB)
$kids = Get-CimInstance Win32_Process -Filter "ParentProcessId=$($p.Id)" | Select-Object -ExpandProperty Name
$out.child_processes = @($kids)
Set-Content -LiteralPath "$state\please_stop" -Value 'stop' -NoNewline
$st = [Diagnostics.Stopwatch]::StartNew()
$p.WaitForExit(60000) | Out-Null
$out.stop_s = [math]::Round($st.Elapsed.TotalSeconds,1)
$out.exited = $p.HasExited
Start-Sleep -Seconds 2
$fs = [IO.File]::Open($log, 'Open', 'Read', 'ReadWrite'); $fs.Seek($logStart, 'Begin') | Out-Null
$text = (New-Object IO.StreamReader($fs)).ReadToEnd(); $fs.Close()
$out.index_bytes_after = (Get-Item "$state\index.json").Length
$out.new_set_aside = @(Get-ChildItem $state -Filter 'index.*.bak' | ForEach-Object Name)
$out.fetching_lines = ([regex]::Matches($text, 'fetching \d+ missing piece')).Count
$out.camera_frames = ([regex]::Matches($text, 'camera: one frame taken')).Count
$out.put_aside = ([regex]::Matches($text, 'put the file aside')).Count
$out.backup_failed = ([regex]::Matches($text, 'backup failed')).Count
$out.housekeeping_failed = ([regex]::Matches($text, 'housekeeping failed')).Count
$out.log_tail = @(($text -split "`n") | Where-Object { $_ -match 'icon by the clock|missing pieces|ask|reconcil|interrupted|cut off|stopped cleanly|relief|whisper|asr|backup|fetch' } | Select-Object -First 25 | ForEach-Object { $_.Substring(0, [Math]::Min(200, $_.Length)) })
$bl = "$state\backlog.json"
$out.download_ask = if (Test-Path $bl) { ([regex]::Match((Get-Content -Raw $bl), 'Atlas can fetch[^"]{0,160}')).Value } else { '' }
$out | ConvertTo-Json -Depth 4
