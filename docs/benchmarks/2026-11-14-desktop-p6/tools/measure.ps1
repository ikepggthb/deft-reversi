param([string]$exe, [string]$dir)
Add-Type @"
using System; using System.Runtime.InteropServices;
public class M { [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h); [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte s, int f, int e); }
"@
$env:DEFT_DESKTOP_TRACE = "1"
$home2 = "$dir\home-measure"
Remove-Item -Recurse -Force $home2 -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $home2 | Out-Null
$side = '{"ai":true,"strength":"manual","manual":"level","level":20,"time_ms":5000}'
"{`"sound`":false,`"new_game`":{`"black`":$side,`"white`":$side}}" | Set-Content -Encoding ascii "$home2\settings.json"
$env:DEFT_REVERSI_HOME = $home2

# Startup and idle CPU, three runs (the first is the coldest).
for ($i = 1; $i -le 3; $i++) {
  $err = "$dir\startup-$i.txt"
  $p = Start-Process -FilePath $exe -PassThru -RedirectStandardError $err
  Start-Sleep -Seconds 4
  $p.Refresh(); $c0 = $p.TotalProcessorTime; $t0 = Get-Date
  Start-Sleep -Seconds 10
  $p.Refresh(); $c1 = $p.TotalProcessorTime; $t1 = Get-Date
  $ws = [math]::Round($p.WorkingSet64 / 1MB)
  Stop-Process -Id $p.Id; Start-Sleep -Milliseconds 500
  $cpu = ($c1 - $c0).TotalMilliseconds / ($t1 - $t0).TotalMilliseconds * 100
  "run $i idle_cpu_percent_of_one_core {0:N2} working_set_mib $ws" -f $cpu
  Get-Content $err | Select-String "startup"
}

# AI against AI at level 20, with the animation on.
$err = "$dir\aivsai.txt"
$p = Start-Process -FilePath $exe -PassThru -RedirectStandardError $err
Start-Sleep -Seconds 4
$p.Refresh(); [M]::SetForegroundWindow($p.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 300
[M]::keybd_event(17,0,0,0); [M]::keybd_event(78,0,0,0); [M]::keybd_event(78,0,2,0); [M]::keybd_event(17,0,2,0)
Start-Sleep -Milliseconds 500
[M]::keybd_event(13,0,0,0); [M]::keybd_event(13,0,2,0)
Start-Sleep -Seconds 45
Stop-Process -Id $p.Id
"done"
