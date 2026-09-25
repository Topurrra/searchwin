#Requires -Version 7
<#
Drive the Search you already have open, from the shell.

    ./bench.ps1 tabs                       every tab, yours and the bench's
    ./bench.ps1 open URL                   a bench tab at the end of the row → its id
    ./bench.ps1 go ID URL                  send a bench tab somewhere else
    ./bench.ps1 wait ID [SECONDS]          until the page has loaded
    ./bench.ps1 sleep ID                   put a tab to sleep now, or say what keeps it awake
    ./bench.ps1 select ID                  bring a tab to the front — --test runs only
    ./bench.ps1 resize W H [STEPS]         take the window to W×H in steps, like a drag — --test runs only
    ./bench.ps1 key ID TEXT                press keys on a tab as real key events — --test runs only
    ./bench.ps1 press VK [MODS…]           press a key on the whole app: a Windows virtual-key code,
                                           with ctrl shift alt repeat — --test runs only
    ./bench.ps1 tap ID SELECTOR            a real mouse click on an element (or text=Words) — --test runs only
    ./bench.ps1 text ID                    the page's text
    ./bench.ps1 eval ID JS                 run JavaScript, print what it returns
    ./bench.ps1 click ID SELECTOR
    ./bench.ps1 type ID SELECTOR TEXT
    ./bench.ps1 submit ID SELECTOR         the form around SELECTOR
    ./bench.ps1 shot ID [PATH] [WIDTH]     a PNG of the page → its path
    ./bench.ps1 close ID | all             only tabs the bench opened
    ./bench.ps1 probe                      the window's own state: panels, a dialog, the window
    ./bench.ps1 strip PATH                 the row of tabs across the top, as on screen, to a PNG
    ./bench.ps1 column PATH                the column of tabs, as on screen, to a PNG
    ./bench.ps1 space [new NAME [fresh]|go N|delete|swipe DX|hold DX|release|move N]
                                           test run: the spaces, a new one (signed in with the others, or fresh),
                                           switch to the Nth, delete this one,
                                           two fingers DX points sideways over the column (hold: not let go yet),
                                           move this one to Nth
    ./bench.ps1 fish ID back|continue|real|ok
                                           a scam warning's buttons on that tab (brought to the front):
                                           Go back, Continue anyway, Go to the real site, close the quiet line
                                           — --test runs only. `tabs`/`wait` show FishCatcher's verdict as "fish"
    ./bench.ps1 hit X Y                  what a press at X Y (points from the window's top left) lands on
    ./bench.ps1 field TEXT [keys] [enter | pick N] [wait SECONDS]
                                           type into the field (keys: one keystroke at a time, each timed),
                                           the rows once the engine has answered, then Enter or row N pressed
                                           — --test runs only
    ./bench.ps1 ui KEY VALUE               settings/passwords/welcome/history/downloads/bookmarks/hidden on|off,
                                           look light|dark|system, sidebar on|off, spaces on|off, hides on|off,
                                           folded on|off, peek on|off

Needs Settings › General › "Let a script drive Search" switched on in the
running app. IDs are the first characters of a tab's id, as `tabs` prints
them. With --test, talks to a SEARCH_PROBE run (or a Debug build) instead of
the real one; with --world NAME, to a SEARCH_PROBE=NAME run, the world
"Search (NAME)".

Not here yet, beside the Mac's ./bench: the extensions (ext-*) and `film`.
#>

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

function Fail([string]$message) {
    [Console]::Error.WriteLine($message)
    exit 1
}

function Ask([string]$pipe, [hashtable]$request) {
    $client = [System.IO.Pipes.NamedPipeClientStream]::new('.', $pipe, [System.IO.Pipes.PipeDirection]::InOut,
        [System.IO.Pipes.PipeOptions]::CurrentUserOnly)
    try {
        try {
            $client.Connect(2000)
        } catch {
            # Single quotes: PowerShell reads curly double quotes as quotes too.
            Fail 'Search isn''t listening — is it open, with Settings › General › “Let a script drive Search” on?'
        }
        $line = ($request | ConvertTo-Json -Compress -Depth 8) + "`n"
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($line)
        $client.Write($bytes, 0, $bytes.Length)
        $client.Flush()
        $received = [System.IO.MemoryStream]::new()
        $chunk = [byte[]]::new(65536)
        while (($count = $client.Read($chunk, 0, $chunk.Length)) -gt 0) {
            $received.Write($chunk, 0, $count)
            if ([Array]::IndexOf($chunk, [byte]10, 0, $count) -ge 0) { break }
        }
        $text = [System.Text.Encoding]::UTF8.GetString($received.ToArray()).Split("`n", 2)[0]
        if (-not $text) { $text = '{}' }
        return $text | ConvertFrom-Json -Depth 32
    } finally {
        $client.Dispose()
    }
}

function Number([string]$text) {
    [double]::Parse($text, [System.Globalization.CultureInfo]::InvariantCulture)
}

function Whole([string]$text) {
    [int]::Parse($text, [System.Globalization.CultureInfo]::InvariantCulture)
}

function Full([string]$path) {
    [System.IO.Path]::GetFullPath($path, (Get-Location).ProviderPath)
}

function On([string]$text) { $text -in @('on', 'true', '1', 'yes') }

function Usage([string]$text) { Fail "usage: bench.ps1 $text" }

$argv = @($args | ForEach-Object { [string]$_ })

$world = if ($argv -contains '--test') { 'test' } else { $null }
$at = [array]::IndexOf($argv, '--world')
if ($at -ge 0) {
    if ($at + 1 -ge $argv.Count) { Usage '--world NAME COMMAND …' }
    $world = -join ($argv[$at + 1].ToLowerInvariant().ToCharArray() | Where-Object { ($_ -match '[a-z0-9]') -or $_ -eq '-' })
    if ($world -in @('', '1')) { $world = 'test' }
    $argv = @($argv | Select-Object -First $at) + @($argv | Select-Object -Skip ($at + 2))
}
$argv = @($argv | Where-Object { $_ -notin @('--test', '--yes') })

if ($argv.Count -eq 0 -or $argv[0] -in @('-h', '--help', 'help')) {
    $doc = (Get-Content -Raw $PSCommandPath) -split '<#|#>'
    Write-Output $doc[1].Trim()
    exit 0
}

$verb = $argv[0]
$rest = @($argv | Select-Object -Skip 1)
$request = @{ do = $verb }

switch ($verb) {
    'open' {
        if ($rest.Count -ne 1) { Usage 'open URL' }
        $request.url = $rest[0]
    }
    'go' {
        if ($rest.Count -ne 2) { Usage 'go ID URL' }
        $request.id = $rest[0]; $request.url = $rest[1]
    }
    'wait' {
        if ($rest.Count -lt 1) { Usage 'wait ID [SECONDS]' }
        $request.id = $rest[0]
        if ($rest.Count -gt 1) { $request.seconds = Number $rest[1] }
    }
    { $_ -in @('text', 'close', 'sleep', 'select') } {
        if ($rest.Count -ne 1) { Usage "$verb ID" }
        $request.id = $rest[0]
    }
    'eval' {
        if ($rest.Count -ne 2) { Usage 'eval ID JS' }
        $request.id = $rest[0]; $request.js = $rest[1]
    }
    { $_ -in @('click', 'submit', 'tap') } {
        if ($rest.Count -ne 2) { Usage "$verb ID SELECTOR" }
        $request.id = $rest[0]; $request.selector = $rest[1]
    }
    'type' {
        if ($rest.Count -ne 3) { Usage 'type ID SELECTOR TEXT' }
        $request.id = $rest[0]; $request.selector = $rest[1]; $request.text = $rest[2]
    }
    'shot' {
        if ($rest.Count -lt 1) { Usage 'shot ID [PATH] [WIDTH]' }
        $request.id = $rest[0]
        if ($rest.Count -gt 1) { $request.path = Full $rest[1] }
        if ($rest.Count -gt 2) { $request.width = Number $rest[2] }
    }
    'ui' {
        if ($rest.Count -ne 2) { Usage 'ui KEY VALUE' }
        $request[$rest[0]] = if ($rest[0] -eq 'look') { $rest[1] } else { On $rest[1] }
    }
    'press' {
        if ($rest.Count -lt 1) { Usage 'press VIRTUALKEY [ctrl] [shift] [alt] [repeat]' }
        $request.code = Whole $rest[0]
        $request.mods = @($rest | Select-Object -Skip 1)
    }
    'key' {
        if ($rest.Count -ne 2) { Usage 'key ID TEXT' }
        $request.id = $rest[0]; $request.text = $rest[1]
    }
    'resize' {
        if ($rest.Count -notin @(2, 3)) { Usage 'resize WIDTH HEIGHT [STEPS]' }
        $request.width = Number $rest[0]; $request.height = Number $rest[1]
        if ($rest.Count -eq 3) { $request.steps = Whole $rest[2] }
    }
    { $_ -in @('strip', 'column') } {
        if ($rest.Count -ne 1) { Usage "$verb PATH" }
        $request.path = Full $rest[0]
    }
    'space' {
        $what = if ($rest.Count -gt 0) { $rest[0] } else { '' }
        if ($what -eq 'new') {
            $fresh = $rest.Count -gt 1 -and $rest[-1] -eq 'fresh'
            $words = @($rest | Select-Object -Skip 1)
            if ($fresh) { $words = @($words | Select-Object -SkipLast 1) }
            $name = (@($words) -join ' ').Trim()
            $request.action = 'new'; $request.name = if ($name) { $name } else { 'Test' }; $request.fresh = $fresh
        } elseif ($what -eq 'go' -and $rest.Count -eq 2) { $request.action = 'go'; $request.index = Whole $rest[1] }
        elseif ($what -eq 'delete') { $request.action = 'delete' }
        elseif ($what -in @('swipe', 'hold') -and $rest.Count -eq 2) { $request.action = $what; $request.dx = Number $rest[1] }
        elseif ($what -eq 'release') { $request.action = 'release' }
        elseif ($what -eq 'move' -and $rest.Count -eq 2) { $request.action = 'move'; $request.index = Whole $rest[1] }
        elseif ($what) { Usage 'space [new NAME [fresh] | go N | delete | swipe DX | hold DX | release | move N]' }
    }
    'fish' {
        if ($rest.Count -ne 2 -or $rest[1] -notin @('back', 'continue', 'real', 'ok')) { Usage 'fish ID back|continue|real|ok' }
        $request.id = $rest[0]; $request.act = $rest[1]
    }
    'hit' {
        if ($rest.Count -ne 2) { Usage 'hit X Y' }
        $request.x = Number $rest[0]; $request.y = Number $rest[1]
    }
    'field' {
        if ($rest.Count -lt 1) { Usage 'field TEXT [keys] [enter | pick N] [wait SECONDS]' }
        $request.text = $rest[0]
        for ($i = 1; $i -lt $rest.Count; $i++) {
            switch ($rest[$i]) {
                'keys' { $request.keys = $true }
                'enter' { $request.enter = $true }
                'pick' { $request.pick = Whole $rest[++$i] }
                'wait' { $request.seconds = Number $rest[++$i] }
                default { Usage 'field TEXT [keys] [enter | pick N] [wait SECONDS]' }
            }
        }
    }
    'engine' {
        if ($rest.Count -notin @(1, 2)) { Usage 'engine METHOD [PARAMS-JSON]' }
        $request.method = $rest[0]
        if ($rest.Count -eq 2) { $request.params = $rest[1] | ConvertFrom-Json -AsHashtable }
    }
    { $_ -in @('tabs', 'probe') } { }
    default { Fail ('unknown command “' + $verb + '” — see bench.ps1 help') }
}

$answer = Ask ('search-bench' + $(if ($world) { '-' + $world } else { '' })) $request
if ($answer.PSObject.Properties.Name -contains 'error') { Fail "error: $($answer.error)" }

switch ($verb) {
    'text' {
        Write-Output $answer.text
        if ($answer.truncated) { [Console]::Error.WriteLine("`n[… truncated]") }
    }
    'shot' { Write-Output $answer.path }
    'engine' {
        $value = $answer.result
        if ($value -is [string]) { Write-Output $value } else { Write-Output ($value | ConvertTo-Json -Depth 32) }
    }
    'open' { Write-Output $answer.id }
    'eval' {
        $value = $answer.value
        if ($value -is [string]) { Write-Output $value } else { Write-Output ($value | ConvertTo-Json -Depth 32) }
    }
    'tabs' {
        foreach ($tab in @($answer.tabs)) {
            $mark = if ($tab.bench) { '⚗' } elseif ($tab.active) { '●' } else { ' ' }
            $state = if ($tab.loading) { ' …' } elseif ($tab.asleep -or $tab.dozing) { ' z' } else { '' }
            $title = if ($tab.title) { $tab.title } else { '—' }
            $fish = if ($tab.fish -and $tab.fish.level -ne 'low') { "  [$($tab.fish.level) $($tab.fish.score)]" } else { '' }
            Write-Output "$mark $($tab.id)  $title  $($tab.url)$state$fish"
        }
    }
    default { Write-Output ($answer | ConvertTo-Json -Depth 32) }
}
exit 0
