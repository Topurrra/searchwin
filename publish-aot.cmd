@echo off
rem Native AOT needs the MSVC linker: Visual Studio 2022 (or its Build Tools)
rem with "Desktop development with C++". Run from anywhere; arguments are
rem passed on to dotnet publish.
setlocal
set VS=%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools
if not exist "%VS%\VC\Auxiliary\Build\vcvarsall.bat" set VS=%ProgramFiles%\Microsoft Visual Studio\2022\Community
if not exist "%VS%\VC\Auxiliary\Build\vcvarsall.bat" set VS=%ProgramFiles%\Microsoft Visual Studio\2022\Professional
if not exist "%VS%\VC\Auxiliary\Build\vcvarsall.bat" set VS=%ProgramFiles%\Microsoft Visual Studio\2022\Enterprise
call "%VS%\VC\Auxiliary\Build\vcvarsall.bat" x64 >nul 2>&1
if exist "%USERPROFILE%\.dotnet\dotnet.exe" set "PATH=%USERPROFILE%\.dotnet;%PATH%"
cd /d "%~dp0"
rem Where it goes: -o / --output if given, else dotnet's own publish folder.
set "OUT=Search\bin\x64\Release\net9.0-windows10.0.22621.0\win-x64\publish"
set "NEXT="
for %%a in (%*) do (
    if defined NEXT (
        set "OUT=%%~a"
        set "NEXT="
    ) else (
        if /i "%%~a"=="-o" set "NEXT=1"
        if /i "%%~a"=="--output" set "NEXT=1"
    )
)
dotnet publish Search\Search.csproj -c Release -r win-x64 -p:Platform=x64 -p:PublishAot=true -p:IlcUseEnvironmentalTools=true -p:PublishReadyToRun=false -p:NativeDebugSymbols=false -p:DebugType=none %*
if errorlevel 1 exit /b 1
rem The engine and the tool pages beside Search.exe, so a copy published
rem anywhere (a test run from TEMP) has them: a published Search looks for
rem them only beside itself. The engine is built here first, as build.ps1
rem builds it (with NASM, AVIF encoding uses rav1e's assembly), so an old
rem build lying in target\ is never the one that ships.
set "FEATURES="
set "CARGO="
where nasm >nul 2>&1 && set "FEATURES=--features fast-avif"
if not defined FEATURES if exist "%LOCALAPPDATA%\Programs\nasm\nasm.exe" set "PATH=%LOCALAPPDATA%\Programs\nasm;%PATH%"
if not defined FEATURES if exist "%LOCALAPPDATA%\Programs\nasm\nasm.exe" set "FEATURES=--features fast-avif"
if not defined FEATURES echo No NASM here: the engine's AVIF encoder builds without its assembly (slower).
where cargo >nul 2>&1 && set "CARGO=1"
if defined CARGO pushd Engine
if defined CARGO cargo build --release --no-default-features %FEATURES%
if defined CARGO if errorlevel 1 (popd & echo the engine did not build & exit /b 1)
if defined CARGO popd
rem Without Rust here, only a build at least as new as every engine source
rem is taken (the newer of release and debug, as Search itself picks).
set "ENGINE=Engine\target\release\kil-engine.exe"
if not defined CARGO for /f "usebackq delims=" %%e in (`powershell -NoProfile -Command "$exe = Get-Item Engine\target\release\kil-engine.exe, Engine\target\debug\kil-engine.exe -ErrorAction SilentlyContinue | Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1; if (-not $exe) { 'none' } elseif (Get-ChildItem Engine\src, Engine\compat, Engine\Cargo.toml, Engine\Cargo.lock, Engine\build.rs -Recurse -File | Where-Object LastWriteTimeUtc -gt $exe.LastWriteTimeUtc) { 'stale' } else { $exe.FullName }"`) do set "ENGINE=%%e"
if "%ENGINE%"=="stale" (echo the engine build is older than its sources, and there is no Rust here to build it again & exit /b 1)
if exist "%ENGINE%" (
    copy /y "%ENGINE%" "%OUT%\kil-engine.exe" >nul || exit /b 1
    echo engine: %ENGINE%
)
if exist "Tools\dist\index.html" (
    if exist "%OUT%\tools" rmdir /s /q "%OUT%\tools"
    xcopy "Tools\dist" "%OUT%\tools\" /e /i /y /q >nul || exit /b 1
    echo tools: Tools\dist
)
exit /b 0
