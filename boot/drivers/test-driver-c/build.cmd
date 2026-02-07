@echo off
REM ===========================================================================
REM Build script for test-driver-c using WDK 7600.16385.1
REM
REM Usage: build.cmd
REM
REM Requirements:
REM   - WDK 7600.16385.1 installed (default: C:\WinDDK\7600.16385.1)
REM   - Run from "x64 Free Build Environment" command prompt
REM     OR set WDK_ROOT environment variable
REM ===========================================================================

setlocal enabledelayedexpansion

REM --- Configuration ---
if not defined WDK_ROOT set WDK_ROOT=C:\WinDDK\7600.16385.1

set SRC_DIR=%~dp0src
set BUILD_DIR=%~dp0build
set OUT_NAME=test_driver_c.sys

echo === XenOS C Driver Build ===
echo WDK: %WDK_ROOT%
echo Source: %SRC_DIR%
echo Output: %BUILD_DIR%\%OUT_NAME%
echo.

REM --- Check WDK ---
if not exist "%WDK_ROOT%\bin\x86\amd64\cl.exe" (
    echo ERROR: WDK not found at %WDK_ROOT%
    echo Please install WDK 7600.16385.1 or set WDK_ROOT
    exit /b 1
)

REM --- Create build directory ---
if not exist "%BUILD_DIR%" mkdir "%BUILD_DIR%"

REM --- Set paths ---
set PATH=%WDK_ROOT%\bin\x86\amd64;%PATH%
set INCLUDE=%WDK_ROOT%\inc\ddk;%WDK_ROOT%\inc\api;%WDK_ROOT%\inc\crt
set LIB=%WDK_ROOT%\lib\win7\amd64

echo === Compiling driver.c ===

cl.exe /c /kernel /GS- /Gz /W3 /O2 /Zp8 ^
    /D_AMD64_ /D_WIN64 /DNTDDI_VERSION=0x06010000 /D_WIN32_WINNT=0x0601 ^
    /I"%WDK_ROOT%\inc\ddk" ^
    /I"%WDK_ROOT%\inc\api" ^
    /I"%WDK_ROOT%\inc\crt" ^
    /Fo"%BUILD_DIR%\driver.obj" ^
    "%SRC_DIR%\driver.c"

if errorlevel 1 (
    echo ERROR: Compilation failed
    exit /b 1
)

echo.
echo === Linking %OUT_NAME% ===

link.exe /DRIVER /SUBSYSTEM:NATIVE /ENTRY:DriverEntry /NODEFAULTLIB ^
    /LIBPATH:"%WDK_ROOT%\lib\win7\amd64" ^
    ntoskrnl.lib hal.lib ^
    /OUT:"%BUILD_DIR%\%OUT_NAME%" ^
    "%BUILD_DIR%\driver.obj"

if errorlevel 1 (
    echo ERROR: Linking failed
    exit /b 1
)

echo.
echo === Build complete ===
dir "%BUILD_DIR%\%OUT_NAME%"

echo.
echo To use in XenOS, copy %OUT_NAME% to sysroot\XenOS\system32\drivers\test_driver_c.dll

endlocal

