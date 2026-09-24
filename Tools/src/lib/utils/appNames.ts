/**
 * Shared utilities for displaying the "source app" of clipboard entries
 * (and anywhere else we surface a Windows process name to the user).
 *
 * Why this exists:
 *   - The Win32 process name we capture is often unfriendly (`WINWORD`,
 *     `POWERPNT`, `AcroRd32`, `msedge`). Humans recognize "Microsoft Word"
 *     much faster than "WINWORD" when scanning a clipboard history.
 *   - Two different UI surfaces show the source (the clipboard overlay and
 *     the management tool tile) — they must agree on the mapping or users
 *     get cognitive dissonance switching between them.
 *   - We also assign each app a consistent accent color (hashed from its
 *     name) so the source label is visually distinctive without being a
 *     wall of bright text. The dot encoding is small enough that it doesn't
 *     compete with the content preview for attention.
 */

/**
 * Map raw process names (case-insensitive lookup) to human-readable display
 * names. The keys are lowercased — the `.exe` suffix is stripped upstream
 * in the backend's `get_foreground_process_name()`.
 *
 * Add new entries as users report them — this is curated, not exhaustive.
 */
const APP_NAME_MAP: Record<string, string> = {
    // ─── Microsoft Office / 365 ───
    winword: 'Microsoft Word',
    excel: 'Microsoft Excel',
    powerpnt: 'Microsoft PowerPoint',
    outlook: 'Microsoft Outlook',
    onenote: 'Microsoft OneNote',
    onenotem: 'Microsoft OneNote',
    msaccess: 'Microsoft Access',
    visio: 'Microsoft Visio',
    mspub: 'Microsoft Publisher',
    teams: 'Microsoft Teams',
    'ms-teams': 'Microsoft Teams',
    skype: 'Skype',
    lync: 'Skype for Business',

    // ─── Browsers ───
    chrome: 'Chrome',
    msedge: 'Microsoft Edge',
    firefox: 'Firefox',
    brave: 'Brave',
    opera: 'Opera',
    'opera_gx': 'Opera GX',
    vivaldi: 'Vivaldi',
    iexplore: 'Internet Explorer',
    arc: 'Arc',
    zen: 'Zen Browser',
    librewolf: 'LibreWolf',
    tor: 'Tor Browser',
    safari: 'Safari',

    // ─── Code editors / IDEs ───
    code: 'VS Code',
    'code - insiders': 'VS Code Insiders',
    cursor: 'Cursor',
    devenv: 'Visual Studio',
    idea64: 'IntelliJ IDEA',
    pycharm64: 'PyCharm',
    webstorm64: 'WebStorm',
    phpstorm64: 'PhpStorm',
    rider64: 'Rider',
    clion64: 'CLion',
    goland64: 'GoLand',
    rubymine64: 'RubyMine',
    sublime_text: 'Sublime Text',
    notepad: 'Notepad',
    'notepad++': 'Notepad++',
    'notepad+': 'Notepad++',
    nvim: 'Neovim',
    vim: 'Vim',
    'gvim': 'gVim',
    atom: 'Atom',
    zed: 'Zed',

    // ─── Terminals / shells ───
    windowsterminal: 'Windows Terminal',
    wt: 'Windows Terminal',
    powershell: 'PowerShell',
    pwsh: 'PowerShell',
    cmd: 'Command Prompt',
    'conemu': 'ConEmu',
    'conemu64': 'ConEmu',
    'cmder': 'Cmder',
    'alacritty': 'Alacritty',
    'wezterm-gui': 'WezTerm',
    'tabby': 'Tabby',

    // ─── System / built-in apps ───
    explorer: 'File Explorer',
    taskmgr: 'Task Manager',
    snippingtool: 'Snipping Tool',
    screensketch: 'Snip & Sketch',
    mspaint: 'Paint',
    'paint.net': 'Paint.NET',
    calculator: 'Calculator',
    'searchapp': 'Windows Search',
    'systemsettings': 'Settings',
    'lockapp': 'Lock Screen',
    'shellexperiencehost': 'Windows Shell',
    'startmenuexperiencehost': 'Start Menu',

    // ─── Adobe ───
    acrobat: 'Adobe Acrobat',
    acrord32: 'Adobe Reader',
    acrord64: 'Adobe Reader',
    photoshop: 'Adobe Photoshop',
    illustrator: 'Adobe Illustrator',
    'premiere pro': 'Premiere Pro',
    'after effects': 'After Effects',
    'afterfx': 'After Effects',
    indesign: 'Adobe InDesign',
    audition: 'Adobe Audition',
    bridge: 'Adobe Bridge',
    lightroom: 'Adobe Lightroom',
    xd: 'Adobe XD',

    // ─── Design / creative ───
    figma: 'Figma',
    sketch: 'Sketch',
    gimp: 'GIMP',
    'gimp-2.10': 'GIMP',
    inkscape: 'Inkscape',
    krita: 'Krita',
    blender: 'Blender',
    'affinity photo': 'Affinity Photo',
    'affinity designer': 'Affinity Designer',
    'affinity publisher': 'Affinity Publisher',
    canva: 'Canva',
    obs64: 'OBS Studio',
    obs: 'OBS Studio',

    // ─── Communication / chat ───
    slack: 'Slack',
    discord: 'Discord',
    'discordptb': 'Discord PTB',
    'discordcanary': 'Discord Canary',
    whatsapp: 'WhatsApp',
    telegram: 'Telegram',
    'telegramdesktop': 'Telegram',
    signal: 'Signal',
    zoom: 'Zoom',
    'zoom_meetings': 'Zoom',
    webex: 'Webex',
    thunderbird: 'Thunderbird',
    mailspring: 'Mailspring',

    // ─── Dev tools / DB / API ───
    postman: 'Postman',
    insomnia: 'Insomnia',
    'tableplus': 'TablePlus',
    dbeaver: 'DBeaver',
    'dbeaver-ce': 'DBeaver',
    'datagrip64': 'DataGrip',
    'mongodb compass': 'MongoDB Compass',
    'mongodbcompass': 'MongoDB Compass',
    'mysql workbench': 'MySQL Workbench',
    'sql server management studio': 'SQL Server Management Studio',
    'pgadmin4': 'pgAdmin',
    'docker desktop': 'Docker Desktop',
    'github desktop': 'GitHub Desktop',
    'sourcetree': 'SourceTree',
    'gitkraken': 'GitKraken',
    'tower': 'Tower',
    'fork': 'Fork',

    // ─── Media ───
    vlc: 'VLC',
    mpv: 'mpv',
    spotify: 'Spotify',
    'apple music': 'Apple Music',
    'itunes': 'iTunes',
    'movies & tv': 'Movies & TV',
    'movies_tv': 'Movies & TV',
    'windows media player': 'Windows Media Player',
    'wmplayer': 'Windows Media Player',
    'foobar2000': 'foobar2000',

    // ─── Gaming / launchers ───
    steam: 'Steam',
    steamwebhelper: 'Steam',
    'epicgameslauncher': 'Epic Games',
    'battle.net': 'Battle.net',
    'gog galaxy': 'GOG Galaxy',
    'galaxyclient': 'GOG Galaxy',
    'origin': 'Origin',
    'ea desktop': 'EA App',
    'eaapp': 'EA App',
    'uplay': 'Ubisoft Connect',
    'upc': 'Ubisoft Connect',
    'leagueoflegends': 'League of Legends',
    'leagueclient': 'League of Legends',
    'minecraft launcher': 'Minecraft Launcher',
    'minecraftlauncher': 'Minecraft Launcher',

    // ─── AI / chat assistants ───
    chatgpt: 'ChatGPT',
    claude: 'Claude',
    perplexity: 'Perplexity',
    copilot: 'Microsoft Copilot',

    // ─── Note-taking / productivity ───
    notion: 'Notion',
    obsidian: 'Obsidian',
    'logseq': 'Logseq',
    'roam research': 'Roam Research',
    evernote: 'Evernote',
    'anki': 'Anki',
    'todoist': 'Todoist',
    'things3': 'Things 3',
    'apple notes': 'Apple Notes',

    // ─── Security / privacy ───
    keepassxc: 'KeePassXC',
    keepass: 'KeePass',
    '1password': '1Password',
    bitwarden: 'Bitwarden',
    'bitwarden-desktop': 'Bitwarden',
    dashlane: 'Dashlane',
    'protonpass': 'Proton Pass',
    lastpass: 'LastPass',
    enpass: 'Enpass',
    nordpass: 'NordPass',
    keeper: 'Keeper',
    roboform: 'RoboForm',
    'protonvpn': 'Proton VPN',
    'nordvpn': 'NordVPN',
    'mullvad': 'Mullvad VPN',

    // ─── KeepItLocal itself ───
    keepitlocal: 'KeepItLocal',
};

/**
 * Curated palette for source-app color tagging. Each app deterministically
 * maps to one of these (FNV-ish hash) so the same source always renders in
 * the same color across sessions and across the two windows. Colors are
 * picked from the Tailwind 400 row so they sit comfortably on a dark UI
 * — readable but not saturated enough to compete with the content preview.
 */
const SOURCE_PALETTE: readonly string[] = [
    '#60a5fa', // blue-400
    '#a78bfa', // violet-400
    '#f472b6', // pink-400
    '#fb923c', // orange-400
    '#facc15', // yellow-400
    '#4ade80', // green-400
    '#22d3ee', // cyan-400
    '#f87171', // red-400
    '#c084fc', // purple-400
    '#34d399', // emerald-400
    '#fcd34d', // amber-300
    '#a3e635', // lime-400
];

/**
 * Turn a raw process name into a human-readable display name. Case-
 * insensitive lookup; returns the original value (unchanged) if no entry
 * exists in the map. Empty/null input becomes empty string.
 *
 * Defensive `.exe` strip: persisted history entries from before the backend
 * fix that made the suffix-strip case-insensitive may still contain raw
 * `WINWORD.EXE` / `chrome.exe` strings. We strip the suffix again here so
 * the map lookup and the displayed fallback both look clean. New captures
 * after the backend fix won't have the suffix at all — this just handles
 * the migration window where old entries still live in storage.
 */
export function humanizeAppName(raw: string | null | undefined): string {
    if (!raw) return '';
    const stripped = raw.replace(/\.exe$/i, '');
    return APP_NAME_MAP[stripped.toLowerCase()] ?? stripped;
}

/**
 * Return a stable accent color for a given app name. Same input always
 * returns the same output — uses a fast 32-bit rolling hash, not crypto.
 * Empty/null falls back to the first palette entry.
 */
export function sourceColor(raw: string | null | undefined): string {
    if (!raw) return SOURCE_PALETTE[0];
    // Strip `.exe` first so `WINWORD.EXE` and `WINWORD` hash to the same
    // color — otherwise the visible dot would flicker between two hues as
    // old persisted entries get re-captured.
    const lower = raw.replace(/\.exe$/i, '').toLowerCase();
    let hash = 2166136261; // FNV offset basis, gives good spread on short strings
    for (let i = 0; i < lower.length; i++) {
        hash ^= lower.charCodeAt(i);
        // Math.imul keeps multiplication 32-bit, avoiding float coercion that
        // would otherwise cause different machines to hash differently.
        hash = Math.imul(hash, 16777619);
    }
    return SOURCE_PALETTE[Math.abs(hash) % SOURCE_PALETTE.length];
}
