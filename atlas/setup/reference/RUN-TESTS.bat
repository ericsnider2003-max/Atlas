@echo off
title Atlas tests
setlocal
rem Find atlas.exe whether this file sits beside it or in a subfolder.
rem Walks up rather than assuming one level — a project folder inside a
rem project folder is normal and the launcher should not care.
set "EXE="
for %%D in ("%~dp0." "%~dp0.." "%~dp0..\.." "%~dp0..\..\.." "%~dp0..\..\..\..") do (
  if not defined EXE if exist "%%~fD\atlas.exe" set "EXE=%%~fD\atlas.exe"
)
if not exist "%EXE%" (
  echo Could not find atlas.exe.
  echo Keep this file in the same folder as atlas.exe, or within four folders below it.
  echo Looked in: %~dp0 and four levels above it.
  echo.
  pause
  exit /b 1
)
"%EXE%" --version
echo.
pause
