param([string]$ModelName)
$S = "C:\Users\erics\AppData\Local\Temp\claude\C--Users-erics-Atlas\d49a1750-6617-48ca-92cd-bebc2e2ecf79\scratchpad\asr"
$W = "C:\Users\erics\AppData\Local\Atlas\tools\whisper"
$lat = @()
foreach ($kind in "u","n") { for ($i=1;$i -le 10;$i++) {
  $f = "{0}{1:00}" -f $kind,$i
  $t = [Diagnostics.Stopwatch]::StartNew()
  & "$W\whisper-cli.exe" -m "$S\models\$ModelName.bin" -f "$S\$f.wav" -otxt -of "$S\cliout_$f" -nt -l en 2>$null | Out-Null
  $lat += $t.ElapsedMilliseconds } }
$s = $lat | Sort-Object
"cli $ModelName : p50=$($s[[int]($s.Count*0.5)]) p95=$($s[[int][Math]::Floor($s.Count*0.95)]) n=$($s.Count)"
