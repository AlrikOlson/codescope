@echo off
setlocal EnableDelayedExpansion

REM --------------------------------------------------------------------------
REM with-msvc.cmd — Wraps a command so that MSVC cl.exe is on PATH.
REM Used by the Makefile to let nvcc find the host compiler on Windows.
REM
REM Usage:  scripts\with-msvc.cmd cargo build --features cuda ...
REM         scripts\with-msvc.cmd npx tauri dev --features cuda ...
REM --------------------------------------------------------------------------

REM --- Locate cl.exe via vswhere (ships with VS 2017+) ---
set "VSWHERE=!ProgramFiles(x86)!\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "!VSWHERE!" (
    echo [with-msvc] vswhere not found — Visual Studio may not be installed 1>&2
    goto :run
)

REM Run vswhere, capture output via temp file to avoid for-loop quoting issues
REM with paths that contain spaces and parentheses.
set "TMPOUT=!TEMP!\codescope-msvc-cl.txt"
"!VSWHERE!" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -find "VC\Tools\MSVC\*\bin\Hostx64\x64\cl.exe" > "!TMPOUT!" 2>nul

set "CL_PATH="
if exist "!TMPOUT!" (
    set /p CL_PATH=<"!TMPOUT!"
    del "!TMPOUT!" >nul 2>nul
)

if not defined CL_PATH (
    echo [with-msvc] cl.exe not found — install "Desktop development with C++" workload 1>&2
    goto :run
)

REM Extract directory from full cl.exe path
for %%F in ("!CL_PATH!") do set "CL_DIR=%%~dpF"

REM Strip trailing backslash
if "!CL_DIR:~-1!"=="\" set "CL_DIR=!CL_DIR:~0,-1!"

echo [with-msvc] cl.exe at !CL_DIR! 1>&2
set "PATH=!CL_DIR!;!PATH!"

:run
REM Fix CRT mismatch: Rust MSVC target uses /MD (dynamic CRT).
REM nvcc defaults to /MT which causes LNK2038 when linking candle-kernels + ort.
set "NVCC_APPEND_FLAGS=-Xcompiler /MD"

REM Execute the actual command
%*
exit /b !ERRORLEVEL!
