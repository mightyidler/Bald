param(
    [string]$TestExecutable,
    [ValidateSet('suite', 'worker', 'maple', 'caption')][string]$Mode = 'suite',
    [int]$Repeat = 1
)

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
if (!$TestExecutable) {
    Push-Location $repo
    try {
        $build = & cargo test --no-run --message-format=json
        if ($LASTEXITCODE) { exit $LASTEXITCODE }
        $TestExecutable = $build | ForEach-Object {
            $item = $_ | ConvertFrom-Json
            if ($item.reason -eq 'compiler-artifact' -and $item.profile.test -and $item.executable) {
                $item.executable
            }
        } | Select-Object -Last 1
    } finally { Pop-Location }
}
$TestExecutable = (Resolve-Path $TestExecutable).Path
if (!(Split-Path $TestExecutable -Leaf).StartsWith('bald-')) {
    throw 'Only the BALD test harness may run through this script.'
}
if ($Repeat -lt 1 -or $Repeat -gt 20) { throw 'Repeat must be between 1 and 20.' }
$arguments = switch ($Mode) {
    'caption' { 'window_manager::windows_tests::caption_refresh_preserves_owner_changes_during_intermediate_frame_callback --exact --test-threads=1' }
    'worker' { 'border_service::tests::worker_process_keeps_ui_and_restores_on_disconnect --exact --ignored --test-threads=1 --nocapture' }
    'maple' { 'window_manager::windows_tests:: --test-threads=1' }
    default { '--test-threads=1' }
}

# Desktop isolation contains test windows and global hooks. It does not isolate
# CPU/GPU load or the system compositor. Never switch to this desktop.
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;

public static class BaldIsolatedTests {
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
$logs = Join-Path $repo 'artifacts/native-tests'
New-Item -ItemType Directory -Force $logs | Out-Null
for ($run = 1; $run -le $Repeat; $run++) {
    $log = Join-Path $logs "$Mode-$(Get-Date -Format yyyyMMdd-HHmmss)-$run.log"
    $code = [BaldIsolatedTests]::Run($TestExecutable, $arguments, $repo, $log)
    Get-Content -LiteralPath $log
    if ($code) { exit $code }
}
