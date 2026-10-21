$ErrorActionPreference = 'Stop'
$A = 'C:\Users\erics\AppData\Local\Atlas'
if (Get-Process -Name atlas -ErrorAction SilentlyContinue) { throw 'Atlas is running; not touching program files.' }
$rollbackSrc = Join-Path $A 'atlas.exe.before-merge-20261021'
$rollback = Join-Path $A 'atlas-0.1.0-dev.previous'
$out = [ordered]@{}
$out.current_sha256 = (Get-FileHash (Join-Path $A 'atlas.exe') -Algorithm SHA256).Hash
# The one rollback, under the name Atlas's own Updates page and `atlas update undo` look for.
Copy-Item $rollbackSrc $rollback -Force
$out.rollback = $rollback
$out.rollback_sha256 = (Get-FileHash $rollback -Algorithm SHA256).Hash
if ($out.rollback_sha256 -ne (Get-FileHash $rollbackSrc -Algorithm SHA256).Hash) { throw 'rollback copy mismatch' }
Add-Type -AssemblyName Microsoft.VisualBasic
$recycled = @()
$candidates = Get-ChildItem $A -File | Where-Object {
  ($_.Name -like 'atlas.exe.*' -or $_.Name -like 'atlas-*.exe' -or $_.Name -like 'atlas-*.previous*') -and
  $_.FullName -ne $rollback -and $_.Name -ne 'atlas.exe'
}
foreach ($f in $candidates) {
  $recycled += [ordered]@{ name = $f.Name; bytes = $f.Length; modified = $f.LastWriteTime.ToString('s'); sha256 = (Get-FileHash $f.FullName -Algorithm SHA256).Hash }
  [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile($f.FullName, 'OnlyErrorDialogs', 'SendToRecycleBin')
}
$out.recycled_count = $recycled.Count
$out.recycled_mb = [math]::Round((($recycled | ForEach-Object { $_.bytes }) | Measure-Object -Sum).Sum / 1MB, 1)
$out.left = @(Get-ChildItem $A -File | Where-Object { $_.Name -like 'atlas*' } | ForEach-Object { "$($_.Name) $([math]::Round($_.Length/1MB,1)) MB" })
$out.recycled = $recycled
$out | ConvertTo-Json -Depth 4
