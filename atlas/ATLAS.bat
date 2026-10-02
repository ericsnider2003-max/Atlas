@echo off
title Atlas
setlocal enabledelayedexpansion
cd /d "%~dp0"

rem ---------------------------------------------------------------------
rem  The only file you need. Everything else is reachable from here.
rem
rem  Written this way because a folder of fourteen batch files with no
rem  obvious first one is worse than no launcher at all — you end up
rem  opening them at random, which is exactly what happened.
rem ---------------------------------------------------------------------

rem  EXE is resolved ONCE, to an absolute path, and every launch below goes
rem  through it.
rem
rem  What this replaced: a bare `atlas.exe` on every line, after a loop that
rem  walked the working directory up to four levels looking for one. Two
rem  things went wrong with that. The walk leaves the working directory
rem  somewhere else, so after it every `atlas.exe` resolved against a folder
rem  that is not this file's -- which Atlas answered, and which install's
rem  settings you were editing, depended on where you happened to be
rem  standing. And a shortcut or a scheduled task starts in whatever
rem  directory Windows feels like, so it could reach a *different* install
rem  than the one this file sits in and still look like it worked.
rem
rem  `tests/install.rs` and `tests/hub_settings.rs` were both updated to
rem  require `"%EXE%" <subcommand>` in the 17 Sep merge; the launcher itself
rem  was not, so the tests shipped red against the file they describe.
set "EXEDIR=%~dp0"
if "!EXEDIR:~-1!"=="\" set "EXEDIR=!EXEDIR:~0,-1!"
if not exist "!EXEDIR!\atlas.exe" (
  rem Still search upwards -- someone keeping the launcher in a subfolder is
  rem a real case -- but record the absolute path of what it finds instead of
  rem leaving the answer to the working directory.
  for /l %%i in (1,1,4) do if not exist "!EXEDIR!\atlas.exe" (
    cd ..
    set "EXEDIR=!CD!"
  )
)
set "EXE=!EXEDIR!\atlas.exe"
rem  A fresh copy of the source has no atlas.exe beside this file: cargo puts
rem  it in target\release, which the search above never looks in, so the
rem  launcher used to stop here before its own Build option could run.
rem  Use a build that is already there, or build one if Rust is installed,
rem  and put it beside this file where everything below expects it.
if not exist "!EXE!" (
  set "EXEDIR=%~dp0"
  if "!EXEDIR:~-1!"=="\" set "EXEDIR=!EXEDIR:~0,-1!"
  cd /d "!EXEDIR!"
  if not exist "target\release\atlas.exe" (
    where cargo >nul 2>&1
    if not errorlevel 1 (
      echo.
      echo   There is no atlas.exe yet, so I will build it. The first build takes a while.
      echo.
      cargo build --release
    )
  )
  if exist "target\release\atlas.exe" copy /y "target\release\atlas.exe" "atlas.exe" >nul
  set "EXE=!EXEDIR!\atlas.exe"
)
if not exist "!EXE!" (
  echo.
  echo   Could not find atlas.exe.
  echo   Keep this file in the same folder as atlas.exe, or install Rust from
  echo   https://rustup.rs and run this file again - it will build atlas.exe itself.
  echo.
  pause & exit /b 1
)
rem Stand in the install's own folder, whichever one the search settled on.
rem `models\`, `tools\` and `config\` are all named relative to it below, so
rem the working directory and EXE must agree about which install this is --
rem the whole defect above was that they did not have to.
cd /d "!EXEDIR!"

rem  ATLAS_HOME, because Atlas honours it and this file did not.
rem
rem  `roots::decide()` checks ATLAS_HOME FIRST, ahead of the exe's own folder
rem  -- so with it set, Atlas reads `models\` and `tools\` from there, while
rem  everything below downloaded into the exe's folder. 400MB of models into
rem  a folder Atlas never looks at, and a FIRSTRUN check that kept saying
rem  "not set up yet" after a successful download. `tests/hub_settings.rs:338`
rem  is the assertion.
rem
rem  The program is still started by its own absolute path: ATLAS_HOME says
rem  where Atlas's *home* is, not which binary to run. Same split as
rem  `roots.rs`, which resolves the home independently of `current_exe()`.
set "HOME_DIR=!EXEDIR!"
if defined ATLAS_HOME if not "!ATLAS_HOME!"=="" (
  if not exist "!ATLAS_HOME!" mkdir "!ATLAS_HOME!" 2>nul
  if exist "!ATLAS_HOME!" (
    set "HOME_DIR=!ATLAS_HOME!"
    cd /d "!ATLAS_HOME!"
    echo   ATLAS_HOME is set, so everything goes to !ATLAS_HOME!
  ) else (
    echo   ATLAS_HOME is set to !ATLAS_HOME!, which I could not create.
    echo   Using !EXEDIR! instead, and Atlas will not look there.
    echo.
    pause
  )
)

rem First run? Say so plainly rather than showing a menu to someone who
rem hasn't installed anything yet.
set "FIRSTRUN="
if not exist "models\ggml-base.en.bin" set "FIRSTRUN=1"
if not exist "tools\whisper\whisper-cli.exe" set "FIRSTRUN=1"
rem The speaking half, which this check did not test until 17 Sep 2026. It
rem tested the two hearing files and nothing else, so on a machine where those
rem arrived and the speech download failed, FIRSTRUN went false, the menu
rem stopped offering setup, and Atlas was permanently mute with no prompt to
rem fix it. Every path here is one this script downloads to, a few lines below
rem -- if one moves, both places move together.
if not exist "tools\piper\piper.exe" set "FIRSTRUN=1"
if not exist "models\en_US-amy-medium.onnx" set "FIRSTRUN=1"
if not exist "models\en_US-amy-medium.onnx.json" set "FIRSTRUN=1"

if defined FIRSTRUN (
  echo.
  echo   ================================================================
  echo     Atlas isn't set up yet.
  echo   ================================================================
  echo.
  echo   It needs about 260MB of free, public downloads — no account,
  echo   no key, no card. If anything ever asks for one, stop.
  echo.
  echo   This takes a few minutes. You can close the window and run
  echo   this file again; it picks up where it stopped.
  echo.
  set /p "GO=  Press Enter to set it up, or type N to skip: "
  if /i not "!GO!"=="N" (
    call :setup
    echo.
    echo   Setup finished. Checking what's there...
    echo.
    "%EXE%" doctor
    echo.
    pause
  )
)

:menu
cls
echo.
echo   ATLAS
echo   ---------------------------------------------------------------
echo.
echo     1   Start Atlas                    ^<- this is the one you want
echo.
echo     2   Check what's working           (run this if something's odd)
echo     3   Settings                       (works even when Atlas won't)
echo     4   Who I can sign you in as
echo     5   Set up phone sync
echo.
echo     6   Download anything missing
echo     7   Run the tests
echo.
echo     8   Give Atlas eyes              (faces, objects, hands, reading)
echo.
echo     9   Listen for the wake word       (say its name to start talking)
echo     0   Run in the background          (scheduled work, offers)
echo.
echo     S   Start Atlas when I log in      (then you never open this file)
echo     D   Where to reach it from your phone
echo.
echo     Q   Quit
echo.
set /p "PICK=  Choose (or just press Enter for 1): "

if "%PICK%"=="" goto start
if "%PICK%"=="1" goto start
if "%PICK%"=="2" goto doctor
if "%PICK%"=="3" goto settings
if "%PICK%"=="4" goto access
if "%PICK%"=="5" goto sync
if "%PICK%"=="6" goto getmissing
if "%PICK%"=="7" goto tests
if "%PICK%"=="8" goto geteyes
if "%PICK%"=="9" goto voice
if "%PICK%"=="0" goto daemon
if /i "%PICK%"=="S" goto atlogin
if /i "%PICK%"=="D" goto dashboard
if /i "%PICK%"=="Q" exit /b 0
goto menu

rem  The point of the whole thing: with this on, Atlas is already running
rem  when you sit down, and this file is for setup and repair rather than for
rem  starting anything. It registers a LOGON TASK -- not a service and not an
rem  elevated one. Atlas needs your desktop session, and an assistant that
rem  holds the microphone has no business running as administrator.
rem
rem  `atlas startup on` prints the exact command before it runs it.
:atlogin
cls & echo.
"%EXE%" startup status
echo.
echo   Turn it on with:  atlas startup on
echo   Turn it off with: atlas startup off
echo.
set /p "GO=  Turn it on now? (Y to do it, anything else to leave it): "
if /i "!GO!"=="Y" (
  echo.
  "%EXE%" startup on
)
echo. & pause & goto menu

rem  For the PHONE, and only the phone.
rem
rem  Eric's ruling, 17 Sep 2026: the hub is not a browser. On this machine you
rem  ask Atlas to show you something and it opens its own window -- no browser,
rem  nothing external, which is the same ruling the panels were rebuilt native
rem  for. A phone cannot run that window, so the local web server is how a
rem  phone reaches this desktop, over a VPN that terminates here.
rem
rem  That address was not savable until 17 Sep: the token was regenerated on
rem  every start, so the phone's saved URL broke on every desktop reboot.
rem  `server::token_for` stores it now, so it is the same address for the life
rem  of the install. This entry tells it to you once.
:dashboard
cls & echo.
"%EXE%" hub
echo. & pause & goto menu

:start
cls & echo. & echo   Starting Atlas. Close this window to stop it.
echo.
"%EXE%"
echo. & pause & goto menu

rem  The two doors the program is actually built around, and neither was
rem  reachable from this file until 17 Sep 2026. `main.rs` has handled
rem  `--voice` (:450) and `--daemon` (:446) for as long as they have existed,
rem  but menu item 1 ran the bare typed prompt, so the always-on assistant
rem  could only be started by someone who already knew the flag -- which is
rem  nobody this launcher exists for. `tests/install.rs:205` says so.
rem
rem  Item 1 is left as the default on purpose: which of the three should be
rem  what pressing Enter does is a product ruling, not a test fix.
:voice
cls & echo. & echo   Listening for the wake word. Close this window to stop it.
echo.
"%EXE%" --voice
echo. & pause & goto menu

:daemon
cls & echo. & echo   Running in the background. Close this window to stop it.
echo.
"%EXE%" --daemon
echo. & pause & goto menu

:doctor
cls & "%EXE%" doctor & echo. & pause & goto menu

:settings
cls & echo. & echo   Opening the settings page. & echo.
"%EXE%" settings
echo. & pause & goto menu

:access
cls & "%EXE%" access & echo. & pause & goto menu

:sync
cls & "%EXE%" sync-setup & echo. & pause & goto menu

:getmissing
cls & call :setup & echo. & "%EXE%" doctor & echo. & pause & goto menu

:geteyes
cls
echo.
echo   ================================================================
echo     Giving Atlas eyes
echo   ================================================================
echo.
echo   Eight model files, about 140MB in total, from the OpenCV project.
echo   Free and public, no account, no key, no card. They run inside
echo   Atlas on this machine - no picture is ever sent anywhere.
echo.
echo   Two of the eight read the words on your screen. That one needs
echo   nothing turned on afterwards - try it with:  atlas screen a-picture.png
echo.
echo   Afterwards, turn on "Recognising what it sees" in Settings for the
echo   other six.
echo.
pause
call :seeing
echo.
"%EXE%" doctor
echo. & pause & goto menu

:tests
cls
echo.
echo   Building Atlas and running its tests.
echo.
echo   The first build downloads the libraries Atlas is made of and takes a
echo   while. After that it is quick. Nothing here touches your files or
echo   starts Atlas - it only checks that the code is sound.
echo.
where cargo >nul 2>&1
if errorlevel 1 (
  echo   Cargo isn't on this machine. Atlas needs the Rust toolchain to build
  echo   from source: https://rustup.rs
  echo. & pause & goto menu
)
echo   [1 of 2] Building...
cargo build --release
if errorlevel 1 (
  echo.
  echo   It didn't build. The errors above name the file and the line.
  echo. & pause & goto menu
)
echo.
echo   [2 of 2] Testing...
cargo test
echo. & pause & goto menu

rem ---------------------------------------------------------------------
rem  Setup. Skips anything already present, so it is safe to re-run.
rem ---------------------------------------------------------------------
:setup
rem  The voice pieces, fetched and checked by Atlas itself (`atlas get`):
rem  pinned files, each checked against its SHA-256, resumed if cut off, and
rem  skipped when already here ([have]). This used to be PowerShell downloads
rem  in this file, and on 23 Sep 2026 they could not have worked -- the
rem  listening engine's "latest" address had stopped shipping a Windows
rem  program, and the zip downloads were quoted so the download path never
rem  expanded. See src/getpieces.rs.
"%EXE%" get
exit /b 0

:seeing
rem  The seeing models, fetched and checked by Atlas itself (`atlas get
rem  seeing`): the same eight OpenCV zoo files this used to fetch with no
rem  check at all, now each checked against its SHA-256. See
rem  src/getpieces.rs.
"%EXE%" get seeing
exit /b 0
