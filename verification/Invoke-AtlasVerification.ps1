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

function Invoke-RealAtlasDaemon {
    param(
        [string] $Exe,
        [string] $WorkingDirectory,
        [string] $HomeDirectory,
        [string] $LedgerPath,
        [string] $CandidateHash,
        [string] $Variation,
        [string] $Contract
    )

    $outputPath = Join-Path $HomeDirectory (([guid]::NewGuid()).ToString() + '.daemon.stdout.txt')
    $errorPath = Join-Path $HomeDirectory (([guid]::NewGuid()).ToString() + '.daemon.stderr.txt')
    $started = [DateTime]::UtcNow
    $process = Start-Process -FilePath $Exe -ArgumentList @('--daemon', '--unattended') `
        -WorkingDirectory $WorkingDirectory -RedirectStandardOutput $outputPath `
        -RedirectStandardError $errorPath -PassThru
    Start-Sleep -Seconds 5
    $process.Refresh()
    $alive = -not $process.HasExited
    $workingSet = if ($alive) { (Get-Process -Id $process.Id).WorkingSet64 } else { 0 }
    $exitCode = if ($alive) { $null } else { $process.ExitCode }
    if ($alive) {
        $tree = @($process.Id)
        $frontier = @($process.Id)
        while ($frontier.Count -gt 0) {
            $children = @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
                Where-Object { $_.ParentProcessId -in $frontier } |
                Select-Object -ExpandProperty ProcessId)
            $children = @($children | Where-Object { $_ -notin $tree })
            if ($children.Count -eq 0) { break }
            $tree += $children
            $frontier = $children
        }
        foreach ($childPid in $tree) {
            Stop-Process -Id $childPid -Force -ErrorAction SilentlyContinue
        }
        Wait-Process -Id $process.Id -Timeout 10 -ErrorAction SilentlyContinue
    }
    $process.Close()
    Start-Sleep -Milliseconds 100
    $finished = [DateTime]::UtcNow

    Write-LedgerRecord -LedgerPath $LedgerPath -Record @{
        schema = 'atlas.verification.result.v1'
        run_id = $script:RunId
        candidate_hash = $CandidateHash
        executable = (Resolve-Path -LiteralPath $Exe).Path
        contract = $Contract
        variation = $Variation
        interface = 'real_process'
        status = if ($alive) { 'passed' } else { 'failed' }
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
        expected_pattern = 'daemon remains alive for 5 seconds'
        expected_pattern_matched = $alive
        working_set_bytes = $workingSet
        created_files = @((Split-Path -Leaf $outputPath), (Split-Path -Leaf $errorPath))
        note = 'Started and stopped the real installed Atlas daemon in the disposable ATLAS_HOME; this proves startup liveness and records a working-set sample, not full UI or provider readiness.'
    }
}

function Invoke-RealHubSurface {
    param(
        [string] $Exe,
        [string] $WorkingDirectory,
        [string] $HomeDirectory,
        [string] $LedgerPath,
        [string] $CandidateHash
    )

    $toolsPath = Join-Path $HomeDirectory 'config\tools.yaml'
    $hubPort = Get-Random -Minimum 20000 -Maximum 45000
    $kinPort = $hubPort + 1
    $toolsText = Get-Content -Raw -LiteralPath $toolsPath
    $toolsText = [regex]::Replace($toolsText, '(?m)(^server:\s*\r?\n\s+enabled:\s+true\s*\r?\n\s+port:\s*)8787', "`${1}$hubPort")
    $toolsText = [regex]::Replace($toolsText, '(?m)(^kin:\s*\r?\n\s+enabled:\s+false\s*\r?\n\s+port:\s*)8788', "`${1}$kinPort")
    Set-Content -LiteralPath $toolsPath -Value $toolsText -Encoding UTF8

    $daemonOut = Join-Path $HomeDirectory (([guid]::NewGuid()).ToString() + '.hub-daemon.stdout.txt')
    $daemonErr = Join-Path $HomeDirectory (([guid]::NewGuid()).ToString() + '.hub-daemon.stderr.txt')
    $daemon = Start-Process -FilePath $Exe -ArgumentList @('--daemon', '--unattended') `
        -WorkingDirectory $WorkingDirectory -RedirectStandardOutput $daemonOut `
        -RedirectStandardError $daemonErr -PassThru
    Start-Sleep -Seconds 5
    $daemon.Refresh()
    $alive = -not $daemon.HasExited
    $addressText = if ($alive) { (& $Exe hub address | Out-String) } else { '' }
    $match = [regex]::Match($addressText, 'http://127\.0\.0\.1:\d+/hub\?t=[^\s]+')
    $hubUrl = if ($match.Success) { $match.Value } else { $null }

    $endpoints = @(
        @{ name = 'hub_health_http'; path = ($hubUrl -replace '/hub\?', '/health?'); expected_location = '/health' },
        @{ name = 'hub_page_http'; path = $hubUrl; expected_location = '/hub' },
        @{ name = 'hub_live_json_http'; path = ($hubUrl -replace '/hub\?', '/hub/live.json?'); expected_location = '/hub/live.json' }
    )
    foreach ($endpoint in $endpoints) {
        $started = [DateTime]::UtcNow
        $responsePath = Join-Path $HomeDirectory (([guid]::NewGuid()).ToString() + '.hub-response.txt')
        $statusCode = 0
        $body = ''
        $errorText = ''
        $location = ''
        $sessionCookie = $false
        if ($null -ne $endpoint.path) {
            $client = $null
            $request = $null
            $httpResponse = $null
            try {
                $handler = [System.Net.Http.HttpClientHandler]::new()
                $handler.AllowAutoRedirect = $false
                $client = [System.Net.Http.HttpClient]::new($handler)
                $client.Timeout = [TimeSpan]::FromSeconds(5)
                $request = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Get, $endpoint.path)
                $httpResponse = $client.SendAsync($request).GetAwaiter().GetResult()
                $statusCode = [int]$httpResponse.StatusCode
                $location = [string]$httpResponse.Headers.Location
                $sessionCookie = $httpResponse.Headers.Contains('Set-Cookie')
                $body = $httpResponse.Content.ReadAsStringAsync().GetAwaiter().GetResult()
            } catch {
                $errorText = $_.Exception.Message
            } finally {
                if ($null -ne $httpResponse) { $httpResponse.Dispose() }
                if ($null -ne $request) { $request.Dispose() }
                if ($null -ne $client) { $client.Dispose() }
            }
        } else {
            $errorText = 'The daemon did not publish an authenticated hub address.'
        }
        Set-Content -LiteralPath $responsePath -Value $body -Encoding UTF8
        $finished = [DateTime]::UtcNow
        $matched = ($statusCode -eq 200 -and $sessionCookie -and $body.Contains("url=$($endpoint.expected_location)"))
        Write-LedgerRecord -LedgerPath $LedgerPath -Record @{
            schema = 'atlas.verification.result.v1'
            run_id = $script:RunId
            candidate_hash = $CandidateHash
            executable = (Resolve-Path -LiteralPath $Exe).Path
            contract = $endpoint.name
            variation = 'daemon_live/authenticated_http'
            interface = 'real_process_http'
            status = if ($matched) { 'passed' } else { 'failed' }
            exit_code = if ($statusCode -eq 200) { 0 } else { 1 }
            started_utc = $started.ToString('o')
            finished_utc = $finished.ToString('o')
            duration_ms = [int]($finished - $started).TotalMilliseconds
            stdout_artifact = $responsePath
            stderr_artifact = $daemonErr
            stdout_sha256 = (Get-FileHash -LiteralPath $responsePath -Algorithm SHA256).Hash
            stderr_sha256 = $null
            stdout_bytes = (Get-Item -LiteralPath $responsePath).Length
            stderr_bytes = $null
            expected_pattern = "HTTP 200 with an Atlas session cookie and navigation target $($endpoint.expected_location)"
            expected_pattern_matched = $matched
            http_status = $statusCode
            http_location = $location
            session_cookie_present = $sessionCookie
            error = $errorText
            note = 'Authenticated request to the real installed Atlas Hub listener in a disposable ATLAS_HOME; the token-bearing URL is intentionally not retained in the ledger.'
        }
    }

    if ($alive) {
        $tree = @($daemon.Id)
        $frontier = @($daemon.Id)
        while ($frontier.Count -gt 0) {
            $children = @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
                Where-Object { $_.ParentProcessId -in $frontier } |
                Select-Object -ExpandProperty ProcessId)
            $children = @($children | Where-Object { $_ -notin $tree })
            if ($children.Count -eq 0) { break }
            $tree += $children
            $frontier = $children
        }
        foreach ($childPid in $tree) {
            Stop-Process -Id $childPid -Force -ErrorAction SilentlyContinue
        }
        Wait-Process -Id $daemon.Id -Timeout 10 -ErrorAction SilentlyContinue
    }
    $daemon.Close()
    Start-Sleep -Milliseconds 100
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

    Invoke-RealHubSurface -Exe $resolvedExe -WorkingDirectory $unrelatedDirectory `
        -HomeDirectory $homeDirectory -LedgerPath $ledgerPath -CandidateHash $candidateHash

    # The daemon must stay alive on a fresh profile, and again after its first
    # process is deliberately stopped.  The queue replay after these two
    # starts confirms that the profile remains readable across the restart.
    Invoke-RealAtlasDaemon -Exe $resolvedExe -WorkingDirectory $unrelatedDirectory `
        -HomeDirectory $homeDirectory -LedgerPath $ledgerPath -CandidateHash $candidateHash `
        -Variation 'cold_start/disposable_home' -Contract 'daemon_start_liveness'
    Invoke-RealAtlasDaemon -Exe $resolvedExe -WorkingDirectory $unrelatedDirectory `
        -HomeDirectory $homeDirectory -LedgerPath $ledgerPath -CandidateHash $candidateHash `
        -Variation 'forced_stop/restart' -Contract 'daemon_restart_liveness'

    [void](Invoke-RealAtlasCommand -Exe $resolvedExe -Arguments @('remote', 'list') `
        -WorkingDirectory $unrelatedDirectory -HomeDirectory $homeDirectory -LedgerPath $ledgerPath `
        -CandidateHash $candidateHash -Variation 'after_daemon_restart/public_command' -Contract 'remote_queue_replay_after_daemon' `
        -ExpectedPattern ([regex]::Escape($queueText)))

    # Variation 4: restart the executable and read the durable request back.
    # This checks persistence across processes, not an in-memory handler.
    [void](Invoke-RealAtlasCommand -Exe $resolvedExe -Arguments @('remote', 'list') `
        -WorkingDirectory $unrelatedDirectory -HomeDirectory $homeDirectory -LedgerPath $ledgerPath `
        -CandidateHash $candidateHash -Variation 'restart/public_command' -Contract 'remote_queue_replay' `
        -ExpectedPattern ([regex]::Escape($queueText)))

    # Variations 5–7: drive the same durable request through its running and
    # completed states in separate real processes, then confirm the completed
    # receipt survives one more restart.  The fresh home makes request #1
    # deterministic without relying on the owner's queue.
    [void](Invoke-RealAtlasCommand -Exe $resolvedExe -Arguments @('remote', 'start', '1', '--yes') `
        -WorkingDirectory $unrelatedDirectory -HomeDirectory $homeDirectory -LedgerPath $ledgerPath `
        -CandidateHash $candidateHash -Variation 'restart/state_transition' -Contract 'remote_queue_start' `
        -ExpectedPattern '#1 running')

    [void](Invoke-RealAtlasCommand -Exe $resolvedExe -Arguments @('remote', 'done', '1', 'completed after restart', '--secs', '60') `
        -WorkingDirectory $unrelatedDirectory -HomeDirectory $homeDirectory -LedgerPath $ledgerPath `
        -CandidateHash $candidateHash -Variation 'restart/state_transition' -Contract 'remote_queue_done' `
        -ExpectedPattern 'is done')

    [void](Invoke-RealAtlasCommand -Exe $resolvedExe -Arguments @('remote', 'list') `
        -WorkingDirectory $unrelatedDirectory -HomeDirectory $homeDirectory -LedgerPath $ledgerPath `
        -CandidateHash $candidateHash -Variation 'second_restart/completed_state' -Contract 'remote_queue_completed_replay' `
        -ExpectedPattern '0 waiting, 1 in the queue altogether')

    # A bounded failure must remain visible to the caller, not look like a
    # successful no-op.  The CLI reports this case in text while retaining a
    # zero process exit for interactive use, so the observable pattern is the
    # acceptance condition here.
    [void](Invoke-RealAtlasCommand -Exe $resolvedExe -Arguments @('remote', 'done', '999', 'missing') `
        -WorkingDirectory $unrelatedDirectory -HomeDirectory $homeDirectory -LedgerPath $ledgerPath `
        -CandidateHash $candidateHash -Variation 'invalid_identifier/failure_path' -Contract 'remote_queue_missing_failure' `
        -ExpectedPattern 'No request #999')

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
