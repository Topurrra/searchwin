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
rem The engine and the tool pages beside Search.exe, when they have been
rem built, so a copy published anywhere (a test run from TEMP) has them:
rem a published Search looks for them only beside itself.
set "ENGINE=Engine\target\release\kil-engine.exe"
if not exist "%ENGINE%" set "ENGINE=Engine\target\debug\kil-engine.exe"
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
