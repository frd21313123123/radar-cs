param([string]$ExePath = 'target/x86_64-pc-windows-msvc/release/cas-external-radar.exe')
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path $ExePath).Path
$work = Join-Path ([IO.Path]::GetTempPath()) ('cas-radar-smoke-' + [guid]::NewGuid())
New-Item -ItemType Directory -Force "$work/web","$work/cache" | Out-Null
Copy-Item 'web/index.html' "$work/web/index.html"
# A valid local PNG exercises the native decoder without depending on the CDN.
# Real Nuke upper/lower backgrounds are checked separately with the HTTP client.
Copy-Item 'assets/bomb_ground.png' "$work/cache/de_nuke_radar_psd.png"
Copy-Item 'assets/bomb_ground.png' "$work/cache/de_nuke_lower_radar_psd.png"
$port = Get-Random -Minimum 20000 -Maximum 40000
$process = $null
try {
    $process = Start-Process -FilePath $exe -WorkingDirectory $work -ArgumentList @('--demo','--offline','--no-browser','--no-gsi-install','--port',"$port") -RedirectStandardOutput "$work/stdout.log" -RedirectStandardError "$work/stderr.log" -PassThru
    $token = $null
    for ($i=0; $i -lt 100; $i++) {
        $process.Refresh()
        if ($process.HasExited) { throw 'Radar exited during startup' }
        if (Test-Path "$work/stdout.log") {
            # Start-Process is still writing; explicitly allow its write handle.
            $stream = [IO.File]::Open("$work/stdout.log",[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite)
            $reader = [IO.StreamReader]::new($stream)
            try { $log = $reader.ReadToEnd() } finally { $reader.Dispose() }
            if ($log -match 'token=([0-9a-f]+)') { $token=$Matches[1]; break }
        }
        Start-Sleep -Milliseconds 100
    }
    if (!$token) { throw 'Radar did not provide its local access token' }
    function Request([string]$Path,[string]$Method='GET',$Body=$null) {
        $separator = if ($Path.Contains('?')) { '&' } else { '?' }
        $uri = "http://127.0.0.1:$port$Path${separator}token=$token"
        try {
            if ($null -eq $Body) { return Invoke-RestMethod -Uri $uri -Method $Method -TimeoutSec 10 }
            return Invoke-RestMethod -Uri $uri -Method $Method -ContentType 'application/json' -Body ($Body | ConvertTo-Json -Compress) -TimeoutSec 10
        } catch { throw "Radar HTTP smoke request failed: $Method $Path" }
    }
    function Assert($Condition,[string]$Message) { if (!$Condition) { throw $Message } }
    $state = Request '/api/state'
    Assert ($state.status -eq 'demo' -and $state.players.Count -eq 7) 'Demo players missing'
    # The demo intentionally alternates flashed/clear phases. Check the
    # transported field without requiring a particular startup instant.
    Assert (@($state.players | Where-Object { $_.is_flashed -is [bool] }).Count -eq $state.players.Count) 'Demo flash state missing'
    Assert (($state.grenades | Where-Object { $null -ne $_.remaining_secs }).Count -gt 0) 'Utility timers missing'
    $maps = Request '/api/maps'
    Assert ($maps.maps.de_nuke.radar_info.scale -eq 7) 'Nuke metadata missing'
    $selection = Request '/api/map-selection' 'POST' @{map='de_nuke'}
    Assert ($selection.game_map -eq 'de_nuke' -and $selection.game_map_source -eq 'manual') 'Manual map selection failed'
    Assert ((Request '/api/state').game_map -eq 'de_nuke') 'Shared map selection did not reach snapshots'
    # Unauthorized requests must fail even when the application is healthy.
    $unauthorized = Invoke-WebRequest -Uri "http://127.0.0.1:$port/api/state" -SkipHttpErrorCheck
    Assert ($unauthorized.StatusCode -eq 403) 'Missing access token accepted'
    $svg = Request '/api/weapon-icon?weapon=AK-47'
    Assert ($null -ne $svg) 'Equipment icon missing'
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class RadarSmokeWindow {
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string cls,string title);
 // PowerShell may coerce $null string arguments to empty strings. Pass
 // the wildcard title as a genuine CLR null inside C# instead.
 public static IntPtr RadarWindow() { return FindWindow("CAS_RUST_NATIVE_RADAR_WINDOW", null); }
 [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr hwnd);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
 [DllImport("user32.dll")] public static extern bool GetLayeredWindowAttributes(IntPtr hwnd,out uint key,out byte alpha,out uint flags);
 [DllImport("user32.dll", EntryPoint="GetWindowLongPtrW")] public static extern IntPtr GetWindowLongPtr(IntPtr hwnd,int index);
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessageTimeout(IntPtr hwnd,uint msg,UIntPtr w,IntPtr l,uint flags,uint timeout,out UIntPtr result);
}
'@
    $configured = Request '/api/overlay' 'POST' @{visible=$true; zoom=0.35; opacity=55; show_hp=$false; show_hp_bar=$true;
        show_weapon_icon=$true; show_weapon_name=$false; show_names=$true; show_armor=$true; marker_scale=1.2; text_size=14;
        noise_animation=$true; noise_color='#f2c477'; noise_opacity=65; noise_show_range=$true}
    Assert (!$configured.settings.show_hp -and $configured.settings.show_hp_bar -and !$configured.settings.show_weapon_name -and $configured.settings.show_armor) 'Detailed marker settings were not applied'
    Assert ($configured.defaults.show_hp -and $configured.defaults.show_weapon_name) 'Settings defaults missing'
    $heard = $null
    for ($i=0; $i -lt 60; $i++) {
        $events = @((Request '/api/state').noise_events)
        if ($events.Count -gt 0 -and $events[0].radius_units -eq 1100) { $heard=$events[0]; break }
        Start-Sleep -Milliseconds 50
    }
    Assert ($null -ne $heard -and $heard.position.Count -eq 3 -and $heard.duration_ms -gt 0) 'Sound impulses missing'
    $hwnd = [IntPtr]::Zero
    for ($i=0; $i -lt 100; $i++) {
        $hwnd = [RadarSmokeWindow]::RadarWindow()
        if ($hwnd -ne [IntPtr]::Zero -and [RadarSmokeWindow]::IsWindowVisible($hwnd)) { break }
        Start-Sleep -Milliseconds 100
    }
    Assert ($hwnd -ne [IntPtr]::Zero -and [RadarSmokeWindow]::IsWindowVisible($hwnd)) 'Native window not visible'
    Assert ((Request '/api/overlay').window_ready) 'Native window bridge not ready'
    # Read Windows' actual layered-window alpha, not just the API's echo.
    foreach ($opacity in @(55,10,100,55)) {
        $null = Request '/api/overlay' 'POST' @{opacity=$opacity}
        $expectedAlpha = [byte][Math]::Floor($opacity * 255 / 100)
        $alpha = [byte]0; $key = [uint32]0; $flags = [uint32]0; $applied = $false
        for ($i=0; $i -lt 50; $i++) {
            if ([RadarSmokeWindow]::GetLayeredWindowAttributes($hwnd,[ref]$key,[ref]$alpha,[ref]$flags) -and $alpha -eq $expectedAlpha -and ($flags -band 2) -ne 0) { $applied=$true; break }
            Start-Sleep -Milliseconds 50
        }
        Assert $applied 'Native window transparency did not update'
    }
    $rendered = $false
    for ($i=0; $i -lt 100; $i++) {
        if ((Request '/api/overlay').map_ready) { $rendered=$true; break }
        Start-Sleep -Milliseconds 100
    }
    Assert $rendered 'Native PNG background was not decoded and rendered'
    $style = [RadarSmokeWindow]::GetWindowLongPtr($hwnd,-16).ToInt64()
    Assert (($style -band 0x00C00000) -eq 0) 'Native window still has a Windows caption'
    $result = [UIntPtr]::Zero
    Assert ([RadarSmokeWindow]::SendMessageTimeout($hwnd,0,[UIntPtr]::Zero,[IntPtr]::Zero,2,5000,[ref]$result) -ne [IntPtr]::Zero) 'Native message loop is not responding'
    # The same close path used by the internal X hides, persists, and reopens.
    $null = [RadarSmokeWindow]::SendMessageTimeout($hwnd,0x10,[UIntPtr]::Zero,[IntPtr]::Zero,2,5000,[ref]$result)
    Assert (![RadarSmokeWindow]::IsWindowVisible($hwnd)) 'Native close did not hide window'
    Assert ([RadarSmokeWindow]::IsWindow($hwnd)) 'Native close destroyed window, cannot reopen'
    Assert (!(Request '/api/overlay').settings.visible) 'Native close did not update shared settings'
    $saved = Get-Content "$work/cache/overlay_settings.json" -Raw | ConvertFrom-Json
    Assert (!$saved.visible) 'Native close did not persist visibility'
    Assert (!$saved.show_hp -and $saved.show_names -and $saved.show_armor -and !$saved.show_weapon_name -and $saved.text_size -eq 14) 'Detailed settings were not persisted'
    Assert ($saved.opacity -eq 55) 'Window transparency was not persisted'
    $null = Request '/api/overlay' 'POST' @{visible=$true}
    for ($i=0; $i -lt 50 -and ![RadarSmokeWindow]::IsWindowVisible($hwnd); $i++) { Start-Sleep -Milliseconds 100 }
    Assert ([RadarSmokeWindow]::IsWindowVisible($hwnd)) 'Native reopen failed'
    $null = Request '/api/map-selection' 'POST' @{map=$null}
    Assert ((Request '/api/state').game_map -eq 'de_mirage') 'AUTO did not resume the game map'
    Write-Host 'PASS Windows runtime: HTTP, seven demo players, flash, utility clocks, Nuke selection, authentication, equipment icons, visible borderless native window, actual window transparency and persistence, decoded PNG background, responsive loop, close persistence and reopening.'
} catch {
    $failure = $_
    try { Write-Host ('Overlay failure status: ' + ((Request '/api/overlay') | ConvertTo-Json -Compress -Depth 4)) } catch { Write-Host 'Overlay diagnostic unavailable' }
    if ($null -ne $process) {
        $process.Refresh()
        if (!$process.HasExited) { Stop-Process -Id $process.Id -Force; $process.WaitForExit(5000) | Out-Null }
    }
    if (Test-Path "$work/stderr.log") {
        # Never print stdout, which contains the random access URL.
        Get-Content "$work/stderr.log" | Where-Object { $_ -match '\[overlay\]|panicked|thread.*panic' } | ForEach-Object { Write-Host $_ }
    }
    throw $failure
} finally {
    if ($null -ne $process) {
        $process.Refresh()
        if (!$process.HasExited) { Stop-Process -Id $process.Id -Force }
    }
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}
