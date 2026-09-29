# "Smart App Control blocked a file that may be unsafe"

Nothing is wrong with your machine and nothing is wrong with Atlas. Here is
what's going on and what to do. (Rewritten 23 Sep 2026. An earlier version of
this page said unblocking the zip would "almost certainly" get past Smart App
Control. That was wrong, and the reason is explained below.)

## Two different Windows checks

**SmartScreen** shows the blue box that says "Windows protected your PC". It
only looks at files that carry the "downloaded from the internet" mark, and
you can always get past it with **More info → Run anyway**. Atlas takes that
mark off its installed copy the first time it runs, so you see this box once.

**Smart App Control** (Windows 11 only) is stricter and works differently:

- It checks every program and every piece of code as it loads, whether or
  not it was downloaded.
- It lets a program run only if it's signed with a certificate from
  Microsoft's Trusted Root Program, or if Microsoft's cloud already knows
  the file as safe.
- It has no "Run anyway" button, and Microsoft says outright that there is
  no way to make an exception for one program.

Atlas and its voice tools (whisper, piper) aren't signed. That's why the only
fixes are to get a certificate or to switch Smart App Control off. Unblocking
the zip doesn't get past it, and neither does building Atlas yourself, because
a program you compile is unsigned too. A certificate you make yourself
doesn't count either: it has to come from the Trusted Root Program.

## What to do

**1. Check whether it's even on.** Go to **Windows Security → App & browser
control → Smart App Control settings**. It will say **On**, **Evaluation** or
**Off**. On many machines it's already Off, because in Evaluation it often
switches itself off. Atlas
checks this during setup and tells you under "Worth a look" if it's On or in
Evaluation.

**2. If it's On or in Evaluation, switch it Off** on that same page. Windows
Defender (your antivirus) and SmartScreen stay on. Smart App Control is an
extra layer on top of them.

**3. You can switch it back on later.** Before April 2026, switching it off
was permanent until you reinstalled Windows. The April 2026 update
(KB5083769, builds 26100.8246 / 26200.8246 and later) changed that: you can
now switch it on and off freely. If the page warns that you'd have to
reinstall Windows to turn it back on, you don't have that update yet. Run
Windows Update first.

## Evaluation, specifically

In Evaluation, Smart App Control watches how the machine is used and then
decides by itself whether to switch on. If it switches on after Atlas is set
up, Atlas and its voice tools stop working with no warning. That's why Atlas
tells you about Evaluation too, not only On.

## Without switching it off

The only way is signing, which means paying for it (for example Azure
Artifact Signing at about $10 a month, or a code-signing certificate from a
certificate authority), or distributing Atlas through the Microsoft Store,
which signs the apps it carries. Both are decisions for later. Neither is
needed on a machine where Smart App Control is off.

Sources: [Smart App Control FAQ (Microsoft)](https://support.microsoft.com/en-us/windows/smart-app-control-frequently-asked-questions-285ea03d-fa88-4d56-882e-6698afdb7003) ·
[Smart App Control for developers (Microsoft)](https://learn.microsoft.com/en-us/windows/apps/develop/smart-app-control/overview) ·
[Smart App Control, how it evaluates code (Eric Lawrence)](https://textslashplain.com/2026/04/28/smart-app-control/) ·
[Re-enabling without reinstalling, KB5083769](https://blog-en.topedia.com/2026/04/smart-app-control-in-windows-11-can-now-be-re-enabled-without-reinstalling/)
