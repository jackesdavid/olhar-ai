# Sentinel Edge — Windows installer (card #83, unsigned v1).
#
# Turns a Windows PC into a Sentinel appliance: installs the edge + go2rtc + ffmpeg as a Windows
# SERVICE (auto-start at boot, restart on failure), then opens the pairing page so the person at
# the machine types a code (card #81) — no box token on any command line.
#
# Run from an ELEVATED PowerShell, from the unzipped bundle folder:
#     powershell -ExecutionPolicy Bypass -File .\install.ps1
#
# Unsigned for now: SmartScreen will warn once. There is no Apple-style notarization on Windows;
# removing that warning needs a code-signing cert (deferred).

#Requires -RunAsAdministrator
$ErrorActionPreference = "Stop"

$Here      = $PSScriptRoot
$Base      = Join-Path $env:ProgramData "Sentinel"
$ServiceId = "SentinelEdge"
$ControlUrl = if ($env:SENTINEL_CONTROL_URL) { $env:SENTINEL_CONTROL_URL } else { "https://control.thesentinels.online" }

Write-Host "Sentinel Edge -> $Base"

# --- 1. lay the files down --------------------------------------------------
# %ProgramData% is the right home for a machine-wide service's state: writable by the SYSTEM
# account the service runs as, not casually writable by a standard user.
New-Item -ItemType Directory -Force -Path $Base, "$Base\recordings" | Out-Null
foreach ($f in "sentinel-edge.exe","WinSW.exe") {
    Copy-Item -Force (Join-Path $Here $f) (Join-Path $Base $f)
}

# --- 1b. GStreamer runtime (card #138) --------------------------------------
# The edge LINKS GStreamer — it's the sole media backend now (go2rtc + ffmpeg retired). Install the
# official MSVC runtime so the linked pipeline finds its elements (D3D11/MediaFoundation decode, mux,
# appsink). Idempotent: skip if already present.
$GstRoot = "C:\gstreamer\1.0\msvc_x86_64"
if (-not (Test-Path (Join-Path $GstRoot "bin\gst-inspect-1.0.exe"))) {
    $ver = "1.24.10"
    $msi = Join-Path $env:TEMP "gstreamer-runtime.msi"
    $url = "https://gstreamer.freedesktop.org/data/pkg/windows/$ver/msvc/gstreamer-1.0-msvc-x86_64-$ver.msi"
    Write-Host "Installing GStreamer runtime $ver…"
    curl.exe -sSL -o $msi $url
    Start-Process msiexec.exe -ArgumentList "/i `"$msi`" /qn ADDLOCAL=ALL" -Wait
}

# CONTROL_URL is not a secret (a public hostname), so it ships in the generic bundle. The box's
# identity — its token — is fetched by pairing at first boot; preserve an existing edge.env so an
# upgrade over an enrolled box doesn't wipe its identity.
$EnvFile = Join-Path $Base "edge.env"
if (-not (Test-Path $EnvFile)) {
    "CONTROL_URL=$ControlUrl" | Out-File -Encoding ascii $EnvFile
}

# --- 2. the service, via WinSW ---------------------------------------------
# The edge is a plain console binary, not SCM-aware, so a bare `sc create` would show it as
# "failed to start". WinSW wraps it as a proper service: auto-start, and restart-on-failure — which
# is also how the box comes back after enrollment (the edge exits non-zero on Windows precisely so
# WinSW relaunches it; see enroll.rs) and after the Boxes-page Restart button.
$Path = "$Base;$GstRoot\bin;$env:Path"   # so the edge's linked GStreamer finds its plugin DLLs
$winswXml = @"
<service>
  <id>$ServiceId</id>
  <name>Sentinel Edge</name>
  <description>Sentinel camera appliance — ONVIF discovery, GStreamer media, motion, tracking, recording.</description>
  <executable>$Base\sentinel-edge.exe</executable>
  <workingdirectory>$Base</workingdirectory>
  <onfailure action="restart" delay="3 sec"/>
  <resetfailure>1 hour</resetfailure>
  <log mode="roll-by-size"><sizeThreshold>10240</sizeThreshold><keepFiles>3</keepFiles></log>
  <env name="PATH" value="$Path"/>
  <env name="GST_PLUGIN_PATH" value="$GstRoot\lib\gstreamer-1.0"/>
  <env name="EDGE_CACHE" value="$Base\cameras-cache.json"/>
  <env name="EDGE_ENV_FILE" value="$Base\edge.env"/>
  <env name="EDGE_RECORDING_DIR" value="$Base\recordings"/>
</service>
"@
# No go2rtc/ffmpeg env and no tailnet: the gst-linked edge defaults to GStreamer and reaches the
# cloud outbound-only (SSE commands + live over its own outbound connection, card #144).
$winswXml | Out-File -Encoding utf8 (Join-Path $Base "$ServiceId.xml")

# Reinstall cleanly: stop + uninstall any prior instance first, or `install` fails on a service
# that already exists (the Windows echo of macOS's launchctl bootout-before-bootstrap).
Push-Location $Base
try {
    & ".\WinSW.exe" stop      "$ServiceId.xml" 2>$null | Out-Null
    & ".\WinSW.exe" uninstall "$ServiceId.xml" 2>$null | Out-Null
    Start-Sleep -Seconds 1
    & ".\WinSW.exe" install   "$ServiceId.xml"
    & ".\WinSW.exe" start     "$ServiceId.xml"
} finally { Pop-Location }

# --- 3. firewall: allow the LAN to reach nothing extra ----------------------
# The edge + go2rtc bind localhost; ONVIF discovery is OUTBOUND multicast, which needs no inbound
# rule. So there's deliberately no firewall change here — add one only if a real need shows up on
# a clean machine (noted in the card).

# --- 4. hand the human the pairing page -------------------------------------
# They're standing at this machine; localhost is exactly where to send them. Give the service a
# moment to bind :8788 first.
Start-Sleep -Seconds 3
Start-Process "http://localhost:8788"

Write-Host ""
Write-Host "Installed. The appliance is starting."
Write-Host "A browser should open http://localhost:8788 - enter the pairing code from your Boxes page."
Write-Host "Logs: $Base\$ServiceId.out.log"
