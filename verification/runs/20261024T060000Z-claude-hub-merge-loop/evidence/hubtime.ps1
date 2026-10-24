# Times Hub pages on the real profile with the installed build. The token URL is never printed.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Net.Http
$A = 'C:\Users\erics\AppData\Local\Atlas'
$state = "$A\data\state"
if (Get-Process -Name atlas -ErrorAction SilentlyContinue) { throw 'Atlas already running' }
$p = Start-Process -FilePath "$A\atlas.exe" -ArgumentList @('--daemon') -PassThru -WindowStyle Hidden
$url = $null
for ($i = 0; $i -lt 120 -and -not $url; $i++) {
  Start-Sleep -Milliseconds 500
  $t = (& "$A\atlas.exe" hub address | Out-String)
  $m = [regex]::Match($t, 'http://127\.0\.0\.1:\d+/hub\?t=[^\s]+')
  if ($m.Success) { $url = $m.Value }
}
if (-not $url) { throw 'no hub address' }
$base = ([uri]$url).GetLeftPart([UriPartial]::Authority)
$h = [System.Net.Http.HttpClientHandler]::new(); $h.CookieContainer = [System.Net.CookieContainer]::new(); $h.AllowAutoRedirect = $false
$h.AutomaticDecompression = [System.Net.DecompressionMethods]::GZip
$c = [System.Net.Http.HttpClient]::new($h); $c.Timeout = [TimeSpan]::FromSeconds(20)
$ok = $false
for ($i = 0; $i -lt 60 -and -not $ok; $i++) {
  try { $r = $c.GetAsync($url).GetAwaiter().GetResult(); if ([int]$r.StatusCode -lt 400) { $ok = $true } } catch { Start-Sleep -Seconds 1 }
}
if (-not $ok) { Set-Content -LiteralPath "$state\please_stop" -Value 'stop' -NoNewline; throw 'hub never answered' }
Start-Sleep -Seconds 20   # let start-up work settle in, as a real first look would meet it
$res = [ordered]@{}
foreach ($path in '/hub','/hub/now','/hub/outstanding','/hub/settings','/hub/live.json','/hub/sound') {
  $ms = @(); $codes = @{}
  for ($i = 0; $i -lt 40; $i++) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $r = $c.GetAsync($base + $path).GetAwaiter().GetResult()
    [void]$r.Content.ReadAsByteArrayAsync().GetAwaiter().GetResult()
    $ms += $sw.Elapsed.TotalMilliseconds; $codes[[int]$r.StatusCode] = 1
    Start-Sleep -Milliseconds 250
  }
  $s = $ms | Sort-Object
  $res[$path] = '{0:N1} / {1:N1} / {2:N1} ms  (codes {3})' -f $s[[int]($s.Count*0.5)], $s[[int]($s.Count*0.95)-1], $s[-1], (($codes.Keys) -join ',')
}
Set-Content -LiteralPath "$state\please_stop" -Value 'stop' -NoNewline
$p.WaitForExit(60000) | Out-Null
$res.exited = $p.HasExited
$res | ConvertTo-Json
