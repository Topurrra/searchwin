#Requires -Version 7
<#
The field's regressions, checked on a running test world through bench.ps1:
what Enter does with >commands, !bangs, ?questions, Ctrl+K, sums that are
really names (24/7), and programs found among files. The clipboard's text
is kept and put back (it is never printed).

    $env:SEARCH_PROBE = "myworld"; Start-Process .\Search\bin\...\Search.exe
    ./bench-field.ps1 -World myworld [-Programs C:\Temp\folder]

Needs Settings › General › "Let a script drive Search" (settings.json
{"welcomed":true,"bench":true}). It opens pages (example.com, a YouTube
search), switches the look and puts it back. -Programs names a chosen search
folder holding a file called setup-*.exe: it must be listed and never taken.
Prints one line per check; the exit code is the number that failed.
#>
param(
    [Parameter(Mandatory)] [string]$World,
    [string]$Programs
)
$ErrorActionPreference = 'Stop'
$bench = Join-Path $PSScriptRoot 'bench.ps1'
$failed = 0

# Sums copy their answers: the clipboard's text is put back at the end,
# never shown.
$kept = Get-Clipboard -Raw -ErrorAction SilentlyContinue

function Bench { pwsh -NoProfile -File $bench --world $World @args }
function Field { Bench field @args | ConvertFrom-Json }

function Check([string]$name, [bool]$ok, [string]$saw = '') {
    if ($ok) { "PASS  $name" } else { "FAIL  $name  ($saw)"; $script:failed++ }
}

function Rows($r) { ($r.rows | ForEach-Object { "$($_.kind):$($_.title)" }) -join ' | ' }

# Commands and bangs keep their first row picked, typed or set from the
# browser's side (as Ctrl+L sets it), and Enter takes it.
foreach ($how in 'keys', 'direct') {
    $r = Field '>dark' $how
    Check ">dark ($how) offers the command, picked" ($r.typed -eq '>dark' -and $r.picked -eq 0 -and $r.rows[0].kind -eq 'Command') "$($r.typed) / $($r.picked) / $(Rows $r)"
    $r = Field '>tools' $how
    Check ">tools ($how) offers Tools, picked" ($r.picked -eq 0 -and $r.rows[0].title -eq 'Tools') "$($r.picked) / $(Rows $r)"
    $r = Field '!yt cats' $how
    Check "!yt cats ($how) offers the bang, picked" ($r.typed -eq '!yt cats' -and $r.picked -eq 0 -and $r.rows[0].title -eq 'cats') "$($r.typed) / $($r.picked) / $(Rows $r)"
    $r = Field '? what is this' $how
    Check "? what is this ($how) stays as typed" ($r.typed -eq '? what is this' -and $null -eq ($r.rows | Where-Object kind -eq 'Found')) "$($r.typed) / $(Rows $r)"
}

$look = (Bench probe | ConvertFrom-Json).look
Bench ui look light | Out-Null
$r = Field '>dark' keys enter
Check '>dark Enter runs it' ((Bench probe | ConvertFrom-Json).look -eq 'dark' -and $r.after.typed -eq '') "$($r.after.typed) / $($r.after.active)"
Bench ui look $look | Out-Null

$r = Field '!yt cats' keys enter
Check '!yt cats Enter goes to YouTube' ($r.after.active -like 'https://www.youtube.com/results?search_query=cats*') "$($r.after.active)"

# Ctrl+K: the open page is picked, and Enter switches to it.
$id = Bench open 'https://example.com'
Bench wait $id 10 | Out-Null
$r = Field 'example' ctrlk keys enter
Check 'Ctrl+K example: the open page, picked, switched to' ($r.picked -eq 0 -and $r.rows[0].kind -eq 'Open' -and $r.after.active -like 'https://example.com*') "$($r.picked) / $(Rows $r) / $($r.after.active)"
Bench close $id | Out-Null

# Names and dates that look like sums: words; the answer only on the side.
foreach ($typed in '24/7', '9/11', '7-11', '50/50', '20-20', '1/2') {
    $r = Field $typed enter
    $top = $r.rows | Where-Object top
    Check "$typed is words: nothing copied, a search" ($null -eq $top -and $r.after.said -notlike 'Copied*' -and $r.after.active -like '*google.com*') "$(Rows $r) / $($r.after.said) / $($r.after.active)"
}
foreach ($typed in '24 / 7', '24/7=') {
    $r = Field $typed enter
    Check "$typed is a sum: 3.428571 copied" ($r.after.said -eq 'Copied 3.428571') "$(Rows $r) / $($r.after.said)"
}

if ($Programs) {
    $r = Field 'files: setup' enter
    $program = $r.rows | Where-Object { $_.title -like 'setup*.exe' }
    Check 'files: setup lists the program to show in its folder' ($program -and $program.action -eq 'Reveal' -and -not $program.top) "$(Rows $r)"
    Check 'files: setup Enter takes nothing' ($r.after.said -ne 'Shown in its folder' -and $r.after.typed -eq 'files: setup') "$($r.after.said) / $($r.after.typed)"
}

if ($null -ne $kept) { Set-Clipboard -Value $kept } else { Set-Clipboard -Value $null }
"$failed failed"
exit $failed
