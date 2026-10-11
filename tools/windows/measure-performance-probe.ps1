param(
    [Parameter(Mandatory)][string]$ProbeExecutable,
    [ValidateRange(0,50)][int]$Rules = 50,
    [string]$OutputPath
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$executable = (Resolve-Path -LiteralPath $ProbeExecutable).Path
$allowed = [IO.Path]::GetFullPath((Join-Path $repo 'artifacts/performance-2026-10-10')) + [IO.Path]::DirectorySeparatorChar
if (!$executable.StartsWith($allowed, [StringComparison]::OrdinalIgnoreCase) -or (Split-Path $executable -Leaf) -notlike 'bald-probe-*.exe') {
    throw 'Only a separately prepared performance probe may be launched.'
}
if (![Diagnostics.FileVersionInfo]::GetVersionInfo($executable).ProductName.StartsWith('Bald Probe ')) {
    throw 'The executable metadata is not a prepared probe; refusing to launch a production BALD binary.'
}
$events = [IO.Path]::ChangeExtension($executable, 'events.jsonl')
if (Test-Path -LiteralPath $events) { throw 'Move the previous event log before another run.' }
if (!$OutputPath) { $OutputPath = [IO.Path]::ChangeExtension($executable, 'csv') }
$probe = Start-Process -FilePath $executable -ArgumentList @('--background', "--probe-rules=$Rules") -WindowStyle Hidden -RedirectStandardOutput ([IO.Path]::ChangeExtension($executable, 'stdout.log')) -RedirectStandardError ([IO.Path]::ChangeExtension($executable, 'stderr.log')) -PassThru
$null = $probe.Handle
$clock = [Diagnostics.Stopwatch]::StartNew()
$samples = [Collections.Generic.List[object]]::new()
$cpu = @{}
$mainCpu = 0.0; $webCpu = 0.0
try {
    while (!$probe.HasExited -and $clock.Elapsed.TotalSeconds -lt 150) {
        $processes = @(Get-CimInstance Win32_Process)
        $owned = [Collections.Generic.HashSet[uint32]]::new()
        [void]$owned.Add([uint32]$probe.Id)
        do {
            $changed = $false
            foreach ($process in $processes) {
                if ($owned.Contains([uint32]$process.ParentProcessId) -and $owned.Add([uint32]$process.ProcessId)) { $changed = $true }
            }
        } while ($changed)
        $main = @(); $web = @()
        foreach ($process in $processes) {
            if (!$owned.Contains([uint32]$process.ProcessId)) { continue }
            try { $live = Get-Process -Id $process.ProcessId -ErrorAction Stop; $live.Refresh() } catch { continue }
            $key = "$($process.ProcessId):$($process.CreationDate.Ticks)"
            $total = $live.TotalProcessorTime.TotalMilliseconds
            if ($cpu.ContainsKey($key)) { $delta = [Math]::Max(0, $total - $cpu[$key]) } else { $delta = 0 }
            $cpu[$key] = $total
            $value = [pscustomobject]@{ WorkingSet = $live.WorkingSet64; Private = $live.PrivateMemorySize64; Handles = $live.HandleCount; Threads = $live.Threads.Count }
            if ($process.ProcessId -eq $probe.Id) { $main += $value; $mainCpu += $delta }
            elseif ($process.Name -eq 'msedgewebview2.exe') { $web += $value; $webCpu += $delta }
        }
        $phase = 'background'
        if (Test-Path -LiteralPath $events) {
            foreach ($line in (Get-Content -LiteralPath $events)) {
                try { $event = $line | ConvertFrom-Json; if ($event.kind -eq 'phase') { $phase = $event.data } } catch { }
            }
        }
        $samples.Add([pscustomobject]@{
            Seconds = [Math]::Round($clock.Elapsed.TotalSeconds, 3); Phase = $phase; Rules = $Rules
            MainWorkingSet = ($main | Measure-Object WorkingSet -Sum).Sum; MainPrivate = ($main | Measure-Object Private -Sum).Sum
            WebWorkingSet = ($web | Measure-Object WorkingSet -Sum).Sum; WebPrivate = ($web | Measure-Object Private -Sum).Sum
            MainCpuMs = $mainCpu; WebCpuMs = $webCpu; WebProcesses = $web.Count
            MainThreads = ($main | Measure-Object Threads -Sum).Sum; WebThreads = ($web | Measure-Object Threads -Sum).Sum
            MainHandles = ($main | Measure-Object Handles -Sum).Sum; WebHandles = ($web | Measure-Object Handles -Sum).Sum
        })
        Start-Sleep -Milliseconds 500
        $probe.Refresh()
    }
    if (!$probe.HasExited) { throw 'Probe exceeded its 150-second limit.' }
    if ($probe.ExitCode -ne 0) { throw "Probe exit code: $($probe.ExitCode)" }
} finally {
    $samples | Export-Csv -LiteralPath $OutputPath -NoTypeInformation -Encoding UTF8
    if (!$probe.HasExited) { $probe.Kill(); $probe.WaitForExit(5000) | Out-Null }
    $probe.Dispose()
}
Write-Output $OutputPath
