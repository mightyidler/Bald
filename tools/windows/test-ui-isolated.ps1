param([Parameter(Mandatory)][string]$ProbeExecutable, [switch]$CurrentDesktop, [ValidateSet('lifecycle','tray')][string]$Mode = 'lifecycle')
$ErrorActionPreference='Stop'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$executable=(Resolve-Path -LiteralPath $ProbeExecutable).Path
$allowed=[IO.Path]::GetFullPath((Join-Path $repo 'artifacts/performance-2026-10-10')) + [IO.Path]::DirectorySeparatorChar
if (!$executable.StartsWith($allowed,[StringComparison]::OrdinalIgnoreCase) -or (Split-Path $executable -Leaf) -notlike 'bald-probe-*.exe') { throw 'Only a prepared probe may run in UI isolation.' }
if ($Mode -eq 'tray' -and !$CurrentDesktop) { throw 'Tray notification validation requires Explorer on the current desktop.' }
if (![Diagnostics.FileVersionInfo]::GetVersionInfo($executable).ProductName.StartsWith('Bald Probe ')) { throw 'Refusing to launch a production BALD binary in UI isolation.' }
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;

public static class BaldIsolatedUi {
    [StructLayout(LayoutKind.Sequential)] struct Security {
        public int size; public IntPtr descriptor; public int inherit;
    }
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct Startup {
        public int size; public string reserved; public string desktop; public string title;
        public int x, y, width, height, xchars, ychars, fill, flags;
        public short show, reservedSize; public IntPtr reservedBytes, input, output, error;
    }
    [StructLayout(LayoutKind.Sequential)] struct ProcessInfo {
        public IntPtr process, thread; public int processId, threadId;
    }
    [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern IntPtr CreateDesktopW(string name, IntPtr device, IntPtr mode, int flags, uint access, IntPtr security);
    [DllImport("user32.dll", SetLastError=true)] static extern bool CloseDesktop(IntPtr desktop);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern IntPtr CreateFileW(string path, uint access, uint share, ref Security security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern bool CreateProcessW(string application, StringBuilder command, IntPtr processSecurity, IntPtr threadSecurity, bool inherit, uint flags, IntPtr environment, string directory, ref Startup startup, out ProcessInfo process);
    [DllImport("kernel32.dll", SetLastError=true)] static extern uint WaitForSingleObject(IntPtr handle, uint timeout);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr handle, out uint code);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool TerminateProcess(IntPtr handle, uint code);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);

    public static int Run(string executable, string arguments, string directory, string log) {
        var desktop = CreateDesktopW("Bald.Tests." + Guid.NewGuid().ToString("N"), IntPtr.Zero, IntPtr.Zero, 0, 0x01ff, IntPtr.Zero);
        if (desktop == IntPtr.Zero) throw new Win32Exception();
        IntPtr output = new IntPtr(-1), input = new IntPtr(-1);
        var process = new ProcessInfo();
        try {
            var security = new Security { size = Marshal.SizeOf(typeof(Security)), inherit = 1 };
            output = CreateFileW(log, 0x40000000, 3, ref security, 2, 0x80, IntPtr.Zero);
            if (output == new IntPtr(-1)) throw new Win32Exception();
            input = CreateFileW("NUL", 0x80000000, 3, ref security, 3, 0x80, IntPtr.Zero);
            if (input == new IntPtr(-1)) throw new Win32Exception();
            // Get the name from the handle, rather than assuming WinSta0.
            var name = new StringBuilder(256); int needed;
            if (!GetUserObjectInformationW(desktop, 2, name, name.Capacity * 2, out needed)) throw new Win32Exception();
            var startup = new Startup { size = Marshal.SizeOf(typeof(Startup)), desktop = name.ToString(), flags = 0x100, input = input, output = output, error = output };
            if (!CreateProcessW(executable, new StringBuilder("\"" + executable + "\" " + arguments), IntPtr.Zero, IntPtr.Zero, true, 0x08000000, IntPtr.Zero, directory, ref startup, out process)) throw new Win32Exception();
            if (WaitForSingleObject(process.process, 300000) != 0) throw new TimeoutException("Isolated native tests exceeded five minutes.");
            uint code;
            if (!GetExitCodeProcess(process.process, out code)) throw new Win32Exception();
            return unchecked((int)code);
        } finally {
            if (process.process != IntPtr.Zero) {
                if (WaitForSingleObject(process.process, 0) == 258) { TerminateProcess(process.process, 1); WaitForSingleObject(process.process, 5000); }
                CloseHandle(process.process); CloseHandle(process.thread);
            }
            if (output != new IntPtr(-1)) CloseHandle(output);
            if (input != new IntPtr(-1)) CloseHandle(input);
            CloseDesktop(desktop);
        }
    }
    [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern bool GetUserObjectInformationW(IntPtr handle, int index, StringBuilder information, int length, out int needed);
}
'@
$log=[IO.Path]::ChangeExtension($executable, 'native.log')
$events=[IO.Path]::ChangeExtension($executable, 'events.jsonl')
if ((Test-Path -LiteralPath $log) -or (Test-Path -LiteralPath $events)) { throw 'Use a fresh probe filename to preserve previous evidence.' }
if ($CurrentDesktop) {
    # Only the independently named, settings-isolated probe can take this path.
    $stderr=[IO.Path]::ChangeExtension($executable, 'stderr.log')
    $modeArgument = if ($Mode -eq 'tray') { '--probe-tray' } else { '--probe-lifecycle' }
    $process=Start-Process -FilePath $executable -ArgumentList @('--background','--probe-rules=50',$modeArgument) -WorkingDirectory $repo -WindowStyle Hidden -RedirectStandardOutput $log -RedirectStandardError $stderr -PassThru
    $null = $process.Handle
    try {
        if (!$process.WaitForExit(300000)) { throw 'Current-desktop UI probe exceeded five minutes.' }
        $code=$process.ExitCode
    } finally {
        if (!$process.HasExited) { $process.Kill(); $process.WaitForExit(5000) | Out-Null }
        $process.Dispose()
    }
    Get-Content -LiteralPath $stderr -Encoding UTF8
} else {
    $code=[BaldIsolatedUi]::Run($executable,'--background --probe-rules=50 --probe-lifecycle',$repo,$log)
}
Get-Content -LiteralPath $log -Encoding UTF8
if ($null -eq $code) { throw 'UI probe exit code was unavailable.' }
if ($code -ne 0) { exit $code }
$observed=@(Get-Content -LiteralPath $events -Encoding UTF8 | ForEach-Object { $_ | ConvertFrom-Json })
if ($Mode -eq 'tray') {
    if (@($observed | Where-Object { $_.kind -eq 'tray-result' -and $_.data -eq 'passed' }).Count -ne 1 -or
        @($observed | Where-Object { $_.kind -eq 'tray-cycle' }).Count -ne 3) { throw 'Tray notification validation did not complete three cycles.' }
    Write-Output 'Tray notifications: three single/double-click and release cycles passed.'
    exit 0
}
if (@($observed | Where-Object { $_.kind -eq 'lifecycle-result' -and $_.data -eq 'passed' }).Count -ne 1 -or
    @($observed | Where-Object { $_.kind -eq 'restored-preferences' }).Count -ne 6 -or
    @($observed | Where-Object { $_.kind -eq 'lifecycle-cycle' }).Count -ne 6 -or
    @($observed | Where-Object { $_.kind -in @('error','js-error','reported-error','state-error') }).Count -ne 0) {
    throw 'UI probe exit did not include all six successful preference/lifecycle checks.'
}
Write-Output 'UI lifecycle: six cycles and preference checks passed.'
exit 0
