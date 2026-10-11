param([ValidateRange(1, 20)][int]$Repeat = 1)

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path

# Reviewed allowlist: memory-only algorithms, mock I/O, serialization, and
# synchronization. Never fall back to the full suite or native worker tests.
# Re-review each listed test's call graph when its implementation changes.
$tests = @(
    'border_service::pure_codec_tests::framed_responses_roundtrip_in_memory_including_service_errors'
    'border_service::pure_codec_tests::truncated_or_invalid_frames_fail_without_socket_or_worker'
    'border_service::pure_codec_tests::oversized_frames_fail_before_reading_payload_or_writing_header'
    'icon_cache::tests::modified_executable_invalidates_cached_success_and_failure_without_file_io'
    'icon_cache::tests::simultaneous_requests_extract_once_and_failures_are_cached_briefly'
    'icon_cache::tests::retained_cache_obeys_entry_and_byte_limits'
    'rules::tests::old_automatic_and_missing_modes_migrate_to_blocked'
    'rules::tests::exact_path_is_case_and_separator_insensitive'
    'rules::tests::automatic_application_excludes_same_process_splash_class'
    'rules::tests::filename_is_used_when_path_is_unavailable'
    'rules::tests::class_hint_disambiguates_matching_executable'
    'rules::tests::scan_matching_preserves_path_filename_class_and_rule_order'
    'rules::tests::scan_match_reuse_preserves_large_fixture_results'
    'rules::tests::scan_normalizes_each_rule_and_window_once_for_identical_inputs'
    'startup::tests::manual_open_never_launches_background_task'
    'ui_window_state::tests::minimized_snapshots_use_screen_coordinates_for_recreation_with_top_or_left_taskbars'
    'ui_window_state::tests::restart_argument_roundtrips_all_visibility_states_and_z_order'
    'watcher::tests::elevation_notification_is_independent_of_ui_and_only_sent_on_change'
    'watcher::tests::mouse_burst_keeps_one_wakeup_and_only_latest_coordinates'
    'watcher::tests::scan_bursts_queue_one_scan_until_consumed_and_retry_after_disconnect'
    'web_app::tests::reopening_permits_one_elevation_retry_only_after_completed_failure'
    'web_app::tests::display_preferences_do_not_require_window_or_service_synchronization'
    'web_app::tests::unchanged_state_serializes_without_settings_or_icons'
    'web_app::tests::updates_share_one_slot_and_failure_or_cancellation_releases_it'
    'web_app::tests::failed_save_does_not_apply_candidate'
    'web_app::tests::failed_apply_restores_persisted_and_service_settings'
    'web_app::tests::rollback_failures_are_reported_instead_of_claiming_success'
    'web_app::tests::selection_uses_live_identity_and_accepts_title_changes'
    'window_manager::tests::restoring_frame_and_caption_preserves_intervening_owner_changes'
    'window_manager::tests::crops_transparent_icon_canvas'
    'window_manager::tests::prefers_more_native_icon_detail_over_padded_canvas_size'
    'window_manager::tests::dragging_is_limited_to_the_top_sixteen_physical_pixels'
)

Push-Location $repo
try {
    $build = & cargo test --bin bald --no-run --locked --offline --message-format=json
    if ($LASTEXITCODE -ne 0) {
        $code = $LASTEXITCODE
        $build | ForEach-Object {
            $item = $_ | ConvertFrom-Json
            if ($item.reason -eq 'compiler-message') { Write-Host $item.message.rendered }
        }
        throw "Test compilation failed: $code"
    }
    $executables = @($build | ForEach-Object {
        $item = $_ | ConvertFrom-Json
        if ($item.reason -eq 'compiler-artifact' -and $item.target.name -eq 'bald' -and $item.profile.test -and $item.executable) {
            $item.executable
        }
    })
    if ($executables.Count -ne 1) { throw 'Expected exactly one compiled BALD test harness.' }
    $executable = (Resolve-Path -LiteralPath $executables[0]).Path
    $logDirectory = Join-Path $repo 'artifacts/pure-tests'
    New-Item -ItemType Directory -Path $logDirectory -Force | Out-Null
    $log = Join-Path $logDirectory ("pure-{0}.log" -f (Get-Date -Format 'yyyyMMdd-HHmmss'))
    for ($iteration = 1; $iteration -le $Repeat; $iteration++) {
        foreach ($test in $tests) {
            # The test EXE inherits the application's GUI subsystem. Redirect
            # handles explicitly so a missing test cannot pass without output.
            $start = [System.Diagnostics.ProcessStartInfo]::new($executable)
            $start.UseShellExecute = $false
            $start.CreateNoWindow = $true
            $start.RedirectStandardOutput = $true
            $start.RedirectStandardError = $true
            foreach ($argument in @($test, '--exact', '--test-threads=1')) {
                $start.ArgumentList.Add($argument)
            }
            $process = [System.Diagnostics.Process]::Start($start)
            try {
                $stdout = $process.StandardOutput.ReadToEndAsync()
                $stderr = $process.StandardError.ReadToEndAsync()
                $process.WaitForExit()
                $output = $stdout.GetAwaiter().GetResult() + $stderr.GetAwaiter().GetResult()
                $code = $process.ExitCode
            } finally { $process.Dispose() }
            $output | Out-File -LiteralPath $log -Append -Encoding utf8
            if ($code -ne 0 -or ($output -join "`n") -notmatch '1 passed; 0 failed; 0 ignored;') {
                $output | Write-Host
                throw "Pure test failed or did not execute exactly once: $test (exit $code)"
            }
        }
        Write-Host "Pure tests: $($tests.Count) passed, 0 failed (iteration $iteration/$Repeat)."
    }
    Write-Host "Log: $log"
} finally {
    Pop-Location
}
