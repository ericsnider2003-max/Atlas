param([string]$ModelName, [int]$Port, [string]$Extra = "", [int]$Threads = 2)
$S = "C:\Users\erics\AppData\Local\Temp\claude\C--Users-erics-Atlas\d49a1750-6617-48ca-92cd-bebc2e2ecf79\scratchpad\asr"
$W = "C:\Users\erics\AppData\Local\Atlas\tools\whisper"
$model = "$S\models\$ModelName.bin"
$sent = Get-Content "$S\sentences.txt"
function Norm($t) { $t = $t -replace "b5b","five"; (($t.ToLower() -replace "[^a-z0-9' ]", " ") -replace "\s+", " ").Trim() -split " " | Where-Object { $_ } }
function Wer($ref, $hyp) {
  $r = Norm $ref; $h = Norm $hyp
  $m = $r.Count; $k = $h.Count
  $prev = 0..$k
  for ($i=1;$i -le $m;$i++){
    $cur = New-Object 'int[]' ($k+1); $cur[0]=$i
    for ($j=1;$j -le $k;$j++){
      $c = 0; if ($r[$i-1] -ne $h[$j-1]) {$c = 1}
      $x1 = $prev[$j]+1; $x2 = $cur[$j-1]+1; $x3 = $prev[$j-1]+$c
      $cur[$j] = [Math]::Min([Math]::Min($x1,$x2),$x3)
    }
    $prev = $cur
  }
  return @($prev[$k], $m)
}
$args2 = @("-m",$model,"--host","127.0.0.1","--port",$Port,"-t",$Threads,"-nt","-ng") + ($Extra -split " " | Where-Object {$_})
$sw = [Diagnostics.Stopwatch]::StartNew()
$p = Start-Process -FilePath "$W\whisper-server.exe" -ArgumentList $args2 -PassThru -WindowStyle Hidden -RedirectStandardError "$S\server_$ModelName.log"
while ($true) { try { $c = New-Object Net.Sockets.TcpClient; $c.Connect("127.0.0.1",$Port); $c.Close(); break } catch { Start-Sleep -Milliseconds 50 } }
$load = $sw.ElapsedMilliseconds
function Mem { $q = Get-Process -Id $p.Id; "{0:N0} MB ws / {1:N0} MB private" -f ($q.WorkingSet64/1MB), ($q.PrivateMemorySize64/1MB) }
$memLoad = Mem
$lat = @(); $errs = 0; $words = 0; $memAfter10 = ""; $n = 0
$rows = @()
for ($pass = 0; $pass -lt 3; $pass++) {
  foreach ($kind in "u","n") {
    for ($i = 1; $i -le 10; $i++) {
      $f = "{0}{1:00}.wav" -f $kind, $i
      $t = [Diagnostics.Stopwatch]::StartNew()
      $out = & curl.exe -s --max-time 30 "http://127.0.0.1:$Port/inference" -F "file=@$S\$f" -F "response_format=text" -F "temperature=0.0"
      $ms = $t.ElapsedMilliseconds
      $lat += $ms; $n++
      if ($n -eq 10) { $memAfter10 = Mem }
      if ($pass -eq 0) { $w = Wer $sent[$i-1] ($out -join " "); $rows += [pscustomobject]@{k=$kind;e=$w[0];n=$w[1];hyp=($out -join " ").Trim()} }
    }
  }
}
$memEnd = Mem
$sorted = $lat | Sort-Object
$p50 = $sorted[[int]($sorted.Count*0.5)]; $p95 = $sorted[[int]([Math]::Floor($sorted.Count*0.95))]
Stop-Process -Id $p.Id -Force
foreach ($k in "u","n") { $x = $rows | Where-Object {$_.k -eq $k}; $e = ($x | Measure-Object e -Sum).Sum; $nn = ($x | Measure-Object n -Sum).Sum; "WER $k : {0:P1} ({1}/{2})" -f ($e/$nn), $e, $nn }
"model=$ModelName extra='$Extra' load_ms=$load"
"mem after load: $memLoad"
"mem after 10 req: $memAfter10"
"mem after $n req: $memEnd"
"server latency ms: p50=$p50 p95=$p95 n=$($lat.Count)"
$rows | Where-Object { $_.e -gt 0 } | ForEach-Object { "  miss[$($_.k)] e=$($_.e): $($_.hyp)" }
