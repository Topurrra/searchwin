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
dotnet publish Search\Search.csproj -c Release -r win-x64 -p:Platform=x64 -p:PublishAot=true -p:IlcUseEnvironmentalTools=true -p:PublishReadyToRun=false -p:NativeDebugSymbols=false -p:DebugType=none %*
