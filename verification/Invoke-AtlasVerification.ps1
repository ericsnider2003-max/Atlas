[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $AtlasExe,

    [string] $RunRoot,
    [switch] $KeepArtifacts
)

$ErrorActionPreference = 'Stop'

function Write-LedgerRecord {
    param([hashtable] $Record, [string] $LedgerPath)
    ($Record | ConvertTo-Json -Depth 12 -Compress) | Add-Content -LiteralPath $LedgerPath -Encoding UTF8
}

function Invoke-RealAtlasCommand {
    param(
        [string] $Exe,
        [string[]] $Arguments,
        [string] $WorkingDirectory,
        [string] $HomeDirectory,
        [string] $LedgerPath,
        [string] $CandidateHash,
        [string] $Variation,
        [string] $Contract,
        [string] $ExpectedPattern
    )

    $outputPath = Join-Path $HomeDirectory (([guid]::NewGuid()).ToString() + '.stdout.txt')
    $errorPath = Join-Path $HomeDirectory (([guid]::NewGuid()).ToString() + '.stderr.txt')
    $before = @(Get-ChildItem -LiteralPath $HomeDirectory -Recurse -File -ErrorAction SilentlyContinue |
        ForEach-Object { $_.FullName.Substring($HomeDirectory.Length).TrimStart('\') })
    $started = [DateTime]::UtcNow
    $process = Start-Process -FilePath $Exe -ArgumentList $Arguments -WorkingDirectory $WorkingDirectory `
        -RedirectStandardOutput $outputPath -RedirectStandardError $errorPath -PassThru -Wait
    # Refresh the Process object before reading ExitCode. PowerShell can keep
    # the original snapshot after waiting, which makes a successful child
    # appear to have a null exit code on some Windows builds.
    $process.Refresh()
    $exitCode = $process.ExitCode
    $finished = [DateTime]::UtcNow

    $stdout = if (Test-Path -LiteralPath $outputPath) { Get-Content -Raw -LiteralPath $outputPath } else { '' }
    $stderr = if (Test-Path -LiteralPath $errorPath) { Get-Content -Raw -LiteralPath $errorPath } else { '' }
    $observable = [string]::Concat($stdout, "`n", $stderr)
    $patternMatched = [string]::IsNullOrWhiteSpace($ExpectedPattern) -or
        $observable -match $ExpectedPattern
    $status = if ($exitCode -eq 0 -and $patternMatched) { 'passed' } else { 'failed' }
    $after = @(Get-ChildItem -LiteralPath $HomeDirectory -Recurse -File -ErrorAction SilentlyContinue |
        ForEach-Object { $_.FullName.Substring($HomeDirectory.Length).TrimStart('\') })
    $created = @($after | Where-Object { $_ -notin $before })

    Write-LedgerRecord -LedgerPath $LedgerPath -Record @{
        schema = 'atlas.verification.result.v1'
        run_id = $script:RunId
        candidate_hash = $CandidateHash
        executable = (Resolve-Path -LiteralPath $Exe).Path
        contract = $Contract
        variation = $Variation
        interface = 'real_process'
        status = $status
        exit_code = $exitCode
        started_utc = $started.ToString('o')
        finished_utc = $finished.ToString('o')
        duration_ms = [int]($finished - $started).TotalMilliseconds
        stdout_artifact = $outputPath
        stderr_artifact = $errorPath
        stdout_sha256 = (Get-FileHash -LiteralPath $outputPath -Algorithm SHA256).Hash
        stderr_sha256 = (Get-FileHash -LiteralPath $errorPath -Algorithm SHA256).Hash
        stdout_bytes = (Get-Item -LiteralPath $outputPath).Length
        stderr_bytes = (Get-Item -LiteralPath $errorPath).Length
        expected_pattern = $ExpectedPattern
        expected_pattern_matched = $patternMatched
        created_files = $created
        note = 'Executed the real Atlas executable. This result proves only the named contract; it does not promote rehearsal or source-only evidence.'
    }

    return $exitCode
}

if (-not (Test-Path -LiteralPath $AtlasExe -PathType Leaf)) {
    throw "Atlas executable not found: $AtlasExe"
}

$resolvedExe = (Resolve-Path -LiteralPath $AtlasExe).Path
$candidateHash = (Get-FileHash -LiteralPath $resolvedExe -Algorithm SHA256).Hash
$script:RunId = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ') + '-' + ([guid]::NewGuid().ToString('N').Substring(0, 8))

if ([string]::IsNullOrWhiteSpace($RunRoot)) {
    $RunRoot = Join-Path (Get-Location) 'verification\runs'
}
if (-not (Test-Path -LiteralPath $RunRoot -PathType Container)) {
    $null = New-Item -ItemType Directory -Path $RunRoot -Force
}
$resolvedRunRoot = (Resolve-Path -LiteralPath $RunRoot).Path
$runDirectory = Join-Path $resolvedRunRoot $script:RunId
$homeDirectory = Join-Path $runDirectory 'atlas-home'
$unrelatedDirectory = Join-Path $runDirectory 'unrelated-working-directory'
$null = New-Item -ItemType Directory -Path $homeDirectory, $unrelatedDirectory -Force
$ledgerPath = Join-Path $runDirectory 'results.jsonl'
$manifestPath = Join-Path $runDirectory 'candidate.json'

@{
    schema = 'atlas.verification.candidate.v1'
    run_id = $script:RunId
    candidate_hash = $candidateHash
    executable = $resolvedExe
    created_utc = [DateTime]::UtcNow.ToString('o')
    runner = 'Invoke-AtlasVerification.ps1'
    evidence_boundary = 'real executable in disposable ATLAS_HOME'
} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $manifestPath -Encoding UTF8

$env:ATLAS_HOME = $homeDirectory

try {
    Write-LedgerRecord -LedgerPath $ledgerPath -Record @{
        schema = 'atlas.verification.run.v1'
        run_id = $script:RunId
        candidate_hash = $candidateHash
        executable = $resolvedExe
        status = 'started'
        manifest = $manifestPath
        ledger = $ledgerPath
        started_utc = [DateTime]::UtcNow.ToString('o')
    }

    # Variation 1: executable identity from the real candidate.
    [void](Invoke-RealAtlasCommand -Exe $resolvedExe -Arguments @('--version') -WorkingDirectory $unrelatedDirectory `
        -HomeDirectory $homeDirectory -LedgerPath $ledgerPath -CandidateHash $candidateHash `
        -Variation 'explicit_home/unrelated_working_directory' -Contract 'candidate_identity' `
        -ExpectedPattern '^atlas\s+\S+')

    # Variation 2: machine diagnosis from the real candidate, with the same isolated state.
    [void](Invoke-RealAtlasCommand -Exe $resolvedExe -Arguments @('doctor') -WorkingDirectory $unrelatedDirectory `
        -HomeDirectory $homeDirectory -LedgerPath $ledgerPath -CandidateHash $candidateHash `
        -Variation 'explicit_home/unrelated_working_directory' -Contract 'machine_diagnosis' `
        -ExpectedPattern 'atlas doctor')

    # Variation 3: queue a harmless request through Atlas's public command
    # path. The disposable home makes the state change safe; the assertion is
    # the observable queue receipt, not merely a zero exit code.
    $queueText = 'verification queue request one'
    [void](Invoke-RealAtlasCommand -Exe $resolvedExe -Arguments @('remote', 'ask', $queueText) `
        -WorkingDirectory $unrelatedDirectory -HomeDirectory $homeDirectory -LedgerPath $ledgerPath `
        -CandidateHash $candidateHash -Variation 'fresh_home/public_command' -Contract 'remote_queue_enqueue' `
        -ExpectedPattern 'Queued as #\d+')

    # Variation 4: restart the executable and read the durable request back.
    # This checks persistence across processes, not an in-memory handler.
    [void](Invoke-RealAtlasCommand -Exe $resolvedExe -Arguments @('remote', 'list') `
        -WorkingDirectory $unrelatedDirectory -HomeDirectory $homeDirectory -LedgerPath $ledgerPath `
        -CandidateHash $candidateHash -Variation 'restart/public_command' -Contract 'remote_queue_replay' `
        -ExpectedPattern ([regex]::Escape($queueText)))

    Write-LedgerRecord -LedgerPath $ledgerPath -Record @{
        schema = 'atlas.verification.run.v1'
        run_id = $script:RunId
        candidate_hash = $candidateHash
        status = 'completed'
        completed_utc = [DateTime]::UtcNow.ToString('o')
        manifest = $manifestPath
        ledger = $ledgerPath
    }
}
finally {
    Remove-Item Env:ATLAS_HOME -ErrorAction SilentlyContinue
}

Write-Output (ConvertTo-Json @{
    run_id = $script:RunId
    candidate_hash = $candidateHash
    manifest = $manifestPath
    ledger = $ledgerPath
} -Compress)
