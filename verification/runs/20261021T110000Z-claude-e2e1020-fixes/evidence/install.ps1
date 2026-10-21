$ErrorActionPreference = 'Stop'
$A = 'C:\Users\erics\AppData\Local\Atlas'
$New = 'C:\Users\erics\Atlas\wt-e2e1020\atlas\target-release\release\atlas.exe'
$tag = 'before-e2e1020-20261021'
if (Get-Process -Name atlas -ErrorAction SilentlyContinue) { throw 'Atlas is running; not installing.' }
$out = [ordered]@{}
$out.old_hash = (Get-FileHash "$A\atlas.exe" -Algorithm SHA256).Hash
Copy-Item "$A\atlas.exe" "$A\atlas.exe.$tag" -Force
$out.rollback = "$A\atlas.exe.$tag"
$out.rollback_hash = (Get-FileHash "$A\atlas.exe.$tag" -Algorithm SHA256).Hash
Copy-Item $New "$A\atlas.exe" -Force
$out.new_hash = (Get-FileHash "$A\atlas.exe" -Algorithm SHA256).Hash

# Captions overlay: owner asked for it off; the profile turned it on in a bulk pass (1 Oct).
$settings = "$A\config\settings.yaml"
Copy-Item $settings "$settings.$tag" -Force
$s = Get-Content -Raw $settings
$s2 = [regex]::Replace($s, '(?m)^overlay\.enabled:\s*on\s*$', 'overlay.enabled: off')
[IO.File]::WriteAllText($settings, $s2)
$out.overlay_setting = ([regex]::Match($s2, '(?m)^overlay\.enabled:.*$')).Value

# Smaller resident speech model (quantised locally from the model already here).
$tools = "$A\config\tools.yaml"
Copy-Item $tools "$tools.$tag" -Force
$t = Get-Content -Raw $tools
$t2 = $t.Replace('stt_model: "models/ggml-base.en.bin"', 'stt_model: "models/ggml-base.en-q5_1.bin"')
[IO.File]::WriteAllText($tools, $t2)
$out.stt_model = ([regex]::Match($t2, '(?m)^\s*stt_model:.*$')).Value.Trim()

# Old set-aside index copies: to the Recycle Bin, never hard-deleted.
Add-Type -AssemblyName Microsoft.VisualBasic
$moved = @()
foreach ($f in Get-ChildItem "$A\data\state" -Filter 'index.unreadable.*.json.bak') {
  $moved += [ordered]@{ name = $f.Name; bytes = $f.Length; sha256 = (Get-FileHash $f.FullName -Algorithm SHA256).Hash }
  [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile($f.FullName, 'OnlyErrorDialogs', 'SendToRecycleBin')
}
$out.recycled = $moved
$out.recycled_total_mb = [math]::Round((($moved | ForEach-Object { $_.bytes }) | Measure-Object -Sum).Sum / 1MB, 1)
$out | ConvertTo-Json -Depth 4
