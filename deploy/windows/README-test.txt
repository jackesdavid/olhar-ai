Sentinel Edge for Windows — install
===================================

This turns this PC into a Sentinel camera appliance. It installs a background
service (starts at boot, restarts itself if it stops) and then opens a page
where you connect it to your account with a short code.

INSTALL
-------
1. Right-click the Start button -> "Terminal (Admin)" (or "Windows PowerShell
   (Admin)").
2. cd into this folder, e.g.:
       cd $env:USERPROFILE\Downloads\SentinelEdge
3. Run:
       powershell -ExecutionPolicy Bypass -File .\install.ps1
4. A browser opens http://localhost:8788. In another tab, open your Sentinel
   account -> Settings -> Boxes -> Add a box, and type the code it shows.

That's it. The appliance shows up online in your Boxes page.

NOTE ON THE SECURITY WARNING
----------------------------
This build isn't code-signed yet, so Windows SmartScreen may say "Windows
protected your PC". Click "More info" -> "Run anyway". Signing (which removes
the warning) comes later.

TAILSCALE (only needed for live video)
--------------------------------------
Enrollment, motion and recording work without it. To watch live video remotely,
install Tailscale from https://tailscale.com/download/windows first — the edge
finds it automatically.

UNINSTALL
---------
From an Admin PowerShell in this folder:
       powershell -ExecutionPolicy Bypass -File .\uninstall.ps1

LOGS
----
    C:\ProgramData\Sentinel\SentinelEdge.out.log
