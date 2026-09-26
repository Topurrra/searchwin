@echo off
rem Native AOT needs the MSVC linker: Visual Studio 2022 (or its Build Tools)
rem with "Desktop development with C++". Run from anywhere; arguments are
rem passed on to dotnet publish.
setlocal
set VS=%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools
if not exist "%VS%\VC\Auxiliary\Build\vcvarsall.bat" set VS=%ProgramFiles%\Microsoft Visual Studio\2022\Community
if not exist "%VS%\VC\Auxiliary\Build\vcvarsall.bat" set VS=%ProgramFiles%\Microsoft Visual Studio\2022\Professional
if not exist "%VS%\VC\Auxiliary\Build\vcvarsall.bat" set VS=%ProgramFiles%\Microsoft Visual Studio\2022\Enterprise
if not exist "%VS%\VC\Auxiliary\Build\vcvarsall.bat" (echo Native AOT needs Visual Studio 2022 C++ build tools & exit /b 1)
call "%VS%\VC\Auxiliary\Build\vcvarsall.bat" x64 >nul 2>&1
if errorlevel 1 (echo Could not configure the x64 C++ linker & exit /b 1)
if exist "%USERPROFILE%\.dotnet\dotnet.exe" set "PATH=%USERPROFILE%\.dotnet;%PATH%"
cd /d "%~dp0"
rem Where it goes: -o / --output if given, else dotnet's own publish folder.
set "OUT=Search\bin\x64\Release\net9.0-windows10.0.22621.0\win-x64\publish"
set "NEXT="
set "UNSUPPORTED="
for %%a in (%*) do (
    if /i "%%~a"=="win-arm64" set "UNSUPPORTED=1"
    if /i "%%~a"=="-p:Platform=arm64" set "UNSUPPORTED=1"
    if defined NEXT (
        set "OUT=%%~a"
        set "NEXT="
    ) else (
        if /i "%%~a"=="-o" set "NEXT=1"
        if /i "%%~a"=="--output" set "NEXT=1"
    )
)
if defined UNSUPPORTED (echo Arm64 release packaging is unsupported until the Rust engine has an Arm64 build & exit /b 1)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0build-components.ps1" -Output "%OUT%" -PreflightOnly
if errorlevel 1 exit /b 1
dotnet publish Search\Search.csproj -c Release -r win-x64 -p:Platform=x64 -p:PublishAot=true -p:IlcUseEnvironmentalTools=true -p:PublishReadyToRun=false -p:NativeDebugSymbols=false -p:DebugType=none %*
if errorlevel 1 exit /b 1
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0build-components.ps1" -Output "%OUT%"
if errorlevel 1 exit /b 1
exit /b 0
