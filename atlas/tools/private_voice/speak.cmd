@echo off
setlocal
if "%ATLAS_PRIVATE_VOICE_RUNNER%"=="" (
  echo ATLAS_PRIVATE_VOICE_RUNNER is not configured 1>&2
  exit /b 2
)
"%ATLAS_PRIVATE_VOICE_RUNNER%" %*
