import {
    Activity,
    Archive,
    ArrowLeftRight,
    Bell,
    BookOpen,
    Calculator,
    Clipboard,
    Code,
    Code2,
    Clock3,
    Columns2,
    Database,
    EyeOff,
    FileImage,
    FileLock2,
    FilePenLine,
    FileSearch,
    FileSpreadsheet,
    ScanLine,
    ScanText,
    FileText,
    FileX,
    Files,
    GitMerge,
    Hash,
    ImageIcon,
    Info,
    KeyRound,
    KeySquare,
    Library,
    Mic,
    Lock,
    NotebookPen,
    Pin,
    Pipette,
    QrCode,
    Search,
    Settings,
    ShieldCheck,
    Sparkles,
    Target,
    Timer,
    Trash2,
    type Icon as LucideIcon,
    Users,
    Video,
    Wrench,
    Zap,
} from '@lucide/svelte';

export type Category = 'Privacy' | 'Utils' | 'Document' | 'Development' | 'Image' | 'Media' | 'File' | 'Automation' | 'Focus';
export type AppScreenKind = 'tool' | 'page';
export type AppScreenLoader = () => Promise<{ default: unknown }>;
export type ToolPackId =
    | 'core'
    | 'utils'
    | 'development'
    | 'privacy'
    | 'document'
    | 'image'
    | 'media'
    | 'file'
    | 'automation'
    | 'time-focus';

export interface ToolDocs {
    use: string;
    offline: string;
    tip: string;
}

export interface BaseScreen {
    id: string;
    name: string;
    description: string;
    kind: AppScreenKind;
    loader: AppScreenLoader;
    acceptsSelected?: boolean;
}

export interface ToolScreen extends BaseScreen {
    kind: 'tool';
    category: Category;
    packId?: ToolPackId;
    available: boolean;
    icon: typeof LucideIcon;
    docs?: ToolDocs;
    /** Hide this ONE tool from every user-facing surface, keeping all its code.
     *
     *  The per-tool twin of `HIDDEN_PACK_IDS` (which only hides a whole pack —
     *  too coarse when a single tool needs more polish than the six beside it).
     *  Set it and the tool disappears from the sidebar, category workspace,
     *  and Tool Packs browser; nothing is deleted, so un-hiding is a
     *  one-line change.
     *
     *  Consumers MUST filter on this — the list is `Sidebar.svelte`,
     *  `CategoryWorkspace.svelte`, and `ToolPacks.svelte`. Miss one and the
     *  tool leaks back into that surface. */
    hidden?: boolean;
}

export interface PageScreen extends BaseScreen {
    kind: 'page';
    icon?: typeof LucideIcon;
}

export interface QuickStartCard {
    label: string;
    targetId: string;
    hint: string;
}

export interface BuiltInProfileDefinition {
    id: string;
    name: string;
    description: string;
    includeAll?: boolean;
    includeCategories?: Category[];
    includeToolIds?: string[];
}

export interface ToolPackDefinition {
    id: ToolPackId;
    name: string;
    description: string;
    defaultEnabled: boolean;
    locked?: boolean;
}

export const toolPacks: ToolPackDefinition[] = [
    {
        id: 'core',
        name: 'Core Pack',
        description: 'File Search and required KeepItLocal pages. Always installed.',
        defaultEnabled: true,
        locked: true,
    },
    {
        id: 'utils',
        name: 'Utilities Pack',
        description: 'Hashing, encoders, QR, calculators, passwords, and format helpers.',
        defaultEnabled: false,
    },
    {
        id: 'development',
        name: 'Development Pack',
        description: 'JWT, SQL, regex, diff, JSON, fake data, IDs, and Markdown helpers.',
        defaultEnabled: false,
    },
    {
        id: 'privacy',
        name: 'Privacy Pack',
        description: 'Metadata inspection/sanitizing, redaction, and shredding tools.',
        defaultEnabled: false,
    },
    {
        id: 'document',
        name: 'Documents Pack',
        description: 'Word, CSV, spreadsheet, and document conversion/cleanup tools.',
        defaultEnabled: false,
    },
    {
        id: 'image',
        name: 'Image Pack',
        description: 'Image conversion, compression, resizing, favicons, Base64, and watermarking.',
        defaultEnabled: false,
    },
    {
        id: 'media',
        name: 'Media Pack',
        description: 'Screen recording and local audio/video utilities.',
        defaultEnabled: true,
    },
    {
        id: 'file',
        name: 'File Tools Pack',
        description: 'File manager, archives, cleanup, duplicate finder, and bulk rename workflows.',
        defaultEnabled: false,
    },
    {
        id: 'automation',
        name: 'Automation Pack',
        description: 'Local automation recipe builder.',
        defaultEnabled: false,
    },
    {
        id: 'time-focus',
        name: 'Time & Focus Pack',
        description: 'Focus sessions and local reminders to keep you on task.',
        defaultEnabled: false,
    },
];

export const categoryOrder: Category[] = ['Automation', 'Focus', 'Utils', 'Development', 'Privacy', 'Document', 'Image', 'Media', 'File'];

/* ─── Packs hidden from user-facing surfaces ───────────────────────
   Per the 2026-05-26 Finalizing.md audit (Phase 11), the Automation
   surface is **moved to v2**. We *hide* it from the sidebar Library
   row and the Tool Packs discovery grid — the pack definition, the
   tool screen registration, the Rust commands, and the entire
   `tools/Automation/*` directory all stay. To re-surface in v2:
   remove `'automation'` from this set.

   Consumers: Sidebar.svelte (Library row + Local Tools filter),
   ToolPacks.svelte (discovery grid). Anywhere else that iterates
   `toolPacks` and renders to the user should consult this set. */
export const HIDDEN_PACK_IDS: ReadonlySet<ToolPackId> = new Set<ToolPackId>(['automation']);

export const categoryIcons: Record<Category, typeof LucideIcon> = {
    Utils: Wrench,
    Development: Code,
    Privacy: Lock,
    Document: FileText,
    Image: ImageIcon,
    Media: Video,
    File: Files,
    Automation: Sparkles,
    Focus: Timer,
};

/* ─── Pack identity — single source of truth ───────────────────────
   Pack accent color (icon-tile tint on the Category Workspace header) and
   the human label. Mirrors ToolPacks.svelte's
   PACK_COLORS / the Sidebar's PACK_LABELS; centralized here so the
   workspace pages, sidebar, and pack grid all agree. */
export const packColors: Record<ToolPackId, string> = {
    core: '#10b981',
    utils: '#f59e0b',
    development: '#22c55e',
    privacy: '#ef4444',
    document: '#fb7185',
    image: '#06b6d4',
    media: '#f97316',
    file: '#64748b',
    automation: '#14b8a6',
    'time-focus': '#8b5cf6',
};

export const packLabels: Record<ToolPackId, string> = {
    core: 'Core',
    utils: 'Utilities',
    development: 'Developer Tools',
    privacy: 'Privacy',
    document: 'Documents',
    image: 'Images',
    media: 'Media',
    file: 'File Tools',
    automation: 'Automations',
    'time-focus': 'Time & Focus',
};

export const pageScreens: PageScreen[] = [
    {
        // Keep the stable `home` id for the default route and deep links;
        // the desktop now presents it as a continuity-focused Recent view.
        id: 'home',
        name: 'Recent',
        description: 'Pick up your recently used apps, files, folders, and local tools.',
        kind: 'page',
        icon: Clock3,
        loader: () => import('$lib/tools/TopBar/Home.svelte'),
        acceptsSelected: true,
    },
    {
        id: 'pinned',
        name: 'Pinned',
        description: 'Keep pinned notes and clipboard items within easy reach.',
        kind: 'page',
        icon: Pin,
        loader: () => import('$lib/tools/TopBar/Home.svelte'),
        acceptsSelected: true,
    },
    {
        id: 'about',
        name: 'About',
        description: 'Overview and guided starting points for KeepItLocal.',
        kind: 'page',
        icon: Info,
        loader: () => import('$lib/tools/TopBar/About.svelte'),
        acceptsSelected: true,
    },
    {
        // Command — the merged Search / Clipboard / Voice page (tabs). The
        // legacy ids (file-search, clipboard-history, voice-to-text, snippets)
        // still exist as screens but the router redirects them HERE onto the
        // right tab (see commandTabForId). acceptsSelected so the embedded
        // FileSearch / VoiceToText can route to Settings when needed.
        id: 'command',
        name: 'Command',
        description: 'Search, clipboard, and voice — your core local workflows in one place.',
        kind: 'page',
        icon: Search,
        loader: () => import('$lib/CommandWorkspace.svelte'),
        acceptsSelected: true,
    },
    {
        // Notes — local note-taking. `.ki` files (Markdown + YAML frontmatter)
        // in Documents/KeepItLocal Notes; indexed by the content-search pillar.
        // Manages its own state, so no `selected` prop needed.
        id: 'notes',
        name: 'Notes',
        description: 'Local Markdown notes (.ki) — yours forever, found by Search.',
        kind: 'page',
        icon: NotebookPen,
        loader: () => import('$lib/tools/Notes/Notes.svelte'),
    },
    {
        // Time Tracker — a first-class, always-visible workspace surface (like
        // Notes / Privacy Audit), NOT a default-off pack tool: it's a flagship
        // paid feature and a background capture runs regardless, so it gets its
        // own sidebar entry. Manages its own state (no `selected` prop).
        id: 'time-tracker',
        name: 'Time Tracker',
        description: 'Private, automatic time tracking — see where your hours actually go.',
        kind: 'page',
        icon: Activity,
        loader: () => import('$lib/tools/Focus/TimeTracker.svelte'),
    },
    {
        // Clipboard History was originally registered as a Utils tool but it
        // belongs in the top bar — it's a first-class workflow surface, not
        // a side tool. Lives next to Search / File Search Index in the top
        // chrome and shares their page-screen lifecycle (always installed,
        // no pack toggle, no Utils sidebar listing).
        id: 'clipboard-history',
        name: 'Clipboard History',
        description: 'Searchable history of everything you copy.',
        kind: 'page',
        icon: Clipboard,
        loader: () => import('$lib/tools/Utils/ClipboardHistory.svelte'),
    },
    {
        // Snippets — the headline pro-tier feature. Page screen rather than
        // a Utils tool because users summon it via the clipboard overlay
        // by typing triggers, so it's a peer-level workflow surface to
        // clipboard history rather than a side utility.
        id: 'snippets',
        name: 'Snippets',
        description: 'Reusable templates expanded into any app via clipboard overlay.',
        kind: 'page',
        icon: Code2,
        loader: () => import('$lib/tools/Utils/Snippets.svelte'),
    },
    {
        // My Commands — user-defined palette commands (URL quicklinks, custom
        // bangs, file/folder/app launchers, trusted shell commands). Promoted
        // out of Settings into its own first-class sidebar surface (2026-06-21).
        // The myCommands store owns persistence + cross-window sync; this page
        // is purely a management view, so no `selected` prop is needed.
        id: 'my-commands',
        name: 'My Commands',
        description: 'Keyword shortcuts you run from the command palette — quicklinks, bangs, files, folders, apps, and trusted shell commands.',
        kind: 'page',
        icon: Zap,
        loader: () => import('$lib/tools/Utils/MyCommands.svelte'),
    },
    {
        // Voice to Text — promoted from Utils tool to first-class top-bar
        // page because it's now a workflow surface (auto-arms when
        // overlays open, mic button on file-search + clipboard pages,
        // etc.) rather than a one-off utility.
        id: 'voice-to-text',
        name: 'Voice to Text',
        description: 'Speak — KeepItLocal transcribes locally. Auto-arms with overlays; mic toggle on pages.',
        kind: 'page',
        icon: Mic,
        loader: () => import('$lib/tools/Utils/VoiceToText.svelte'),
        // VoiceToText.svelte takes a `bind:selected` prop so it can
        // route the user to Settings → Voice when the Vosk model
        // isn't configured. Without this flag the parent's router
        // (`+page.svelte`) instantiates the page without `bind:selected`,
        // and the model-setup CTA silently no-ops.
        acceptsSelected: true,
    },
    {
        // Privacy Audit — the v1 flagship. A first-class, always-available
        // top-level surface (its own sidebar entry), NOT a pack tool: the
        // feature that most defines KeepItLocal shouldn't be hidden behind
        // an optional, default-off pack. Runs local, user-initiated scans
        // (Phase 0: microphone & camera) — zero network, no background work.
        id: 'privacy-audit',
        name: 'Privacy Audit',
        description: 'Local, no-network checks of what can see, hear, or read you on this machine.',
        kind: 'page',
        icon: ShieldCheck,
        loader: () => import('$lib/tools/Privacy/PrivacyAudit.svelte'),
    },
    {
        id: 'settings',
        name: 'Settings',
        description: 'Local preferences and system integrations.',
        kind: 'page',
        icon: Settings,
        loader: () => import('$lib/tools/TopBar/Settings.svelte'),
        // Settings binds `selected` to FileSearchIndex (Search Index section)
        // and uses it for in-app navigation, so it MUST be bound to the router —
        // otherwise those writes go to a dead local prop and "Manage index" /
        // back-links throw or no-op.
        acceptsSelected: true,
    },
    {
        id: 'file-search-index',
        name: 'File Search Index',
        description: 'Manage indexed sources, rebuilds, exclusions, and live watcher behavior.',
        kind: 'page',
        icon: Database,
        loader: () => import('$lib/tools/TopBar/FileSearchIndex.svelte'),
        acceptsSelected: true,
    },
    {
        id: 'profiles',
        name: 'Profiles',
        description: 'Manage visible tool collections for different workflows.',
        kind: 'page',
        icon: Users,
        loader: () => import('$lib/tools/TopBar/ProfileManager.svelte'),
    },
    {
        // Tool Packs discovery — what the sidebar's "Tool Packs" item
        // routes to. Distinct from ProfileManager (which the ProfileSwitcher
        // dropdown still uses for managing user-defined profiles). This
        // screen shows every pack + its tools as cards, lets the user click
        // any tool, and routes them either to the tool itself or to
        // Settings → Tool Packs when the pack isn't installed.
        id: 'tool-packs',
        name: 'Tool Packs',
        description: 'Browse all available tool packs and the tools they ship.',
        kind: 'page',
        icon: Library,
        loader: () => import('$lib/tools/TopBar/ToolPacks.svelte'),
        acceptsSelected: true,
    },
    {
        id: 'documentation',
        name: 'Documentation',
        description: 'In-app guidance for KeepItLocal tools and offline workflows.',
        kind: 'page',
        icon: BookOpen,
        loader: () => import('$lib/tools/TopBar/Documentation.svelte'),
        acceptsSelected: true,
    },
    // Search Guide page was retired — its content moved INTO the search
    // overlay as an inline `?` cheatsheet button so users get the help
    // without leaving the overlay. The Documentation page now covers
    // task-oriented examples for everything else.
    {
        id: 'privacy-guide',
        name: 'Privacy Guide',
        description: 'Offline privacy and safety guidance.',
        kind: 'page',
        icon: ShieldCheck,
        loader: () => import('$lib/tools/TopBar/PrivacyGuide.svelte'),
    },
];

export const toolScreens: ToolScreen[] = [
    {
        id: 'reminders',
        name: 'Reminders',
        category: 'Focus',
        packId: 'time-focus',
        description: 'Local one-off reminders that pop a system notification when due.',
        available: true,
        icon: Bell,
        kind: 'tool',
        loader: () => import('$lib/tools/Focus/Reminders.svelte'),
        docs: {
            use: 'Add a quick reminder — "in 20 minutes", "at 3pm", or pick a time.',
            offline: 'Stored locally and checked on-device; nothing is scheduled in the cloud.',
            tip: 'Reminders fire while KeepItLocal is running (it lives in your tray).',
        },
    },
    {
        id: 'focus-mode',
        name: 'Focus Mode',
        category: 'Focus',
        packId: 'time-focus',
        description: 'A focus session that nudges you off blocklisted apps — non-destructive.',
        available: true,
        icon: Target,
        kind: 'tool',
        loader: () => import('$lib/tools/Focus/FocusMode.svelte'),
        docs: {
            use: 'Pick distracting apps, start a session; KeepItLocal nudges + minimizes them.',
            offline: 'Reads only the foreground app locally — never closes or kills anything.',
            tip: 'Switch to an app and use "Block this" to add it without typing its name.',
        },
    },
    {
        id: 'hash-check',
        name: 'Hash Check',
        category: 'Utils',
        description: 'Compute MD5, SHA-1, SHA-256, BLAKE3',
        available: true,
        icon: Hash,
        kind: 'tool',
        loader: () => import('$lib/tools/Utils/HashCheck.svelte'),
        docs: {
            use: 'Verify file integrity or compare file fingerprints.',
            offline: 'Hashing should never require uploading the file you want to verify.',
            tip: 'Use SHA-256 for modern verification. MD5 is mostly for legacy compatibility.',
        },
    },
    {
        id: 'encoders',
        name: 'Encoders',
        category: 'Utils',
        description: 'Base64, Hex, URL, HTML, Binary, ROT13',
        available: true,
        icon: Code,
        kind: 'tool',
        loader: () => import('$lib/tools/Utils/Encoders.svelte'),
        docs: {
            use: 'Encode/decode Base64, URL, Hex, HTML, Binary, and ROT13 text.',
            offline: 'Encoded data can still contain secrets, tokens, URLs, or private payloads.',
            tip: 'Do not paste real tokens or secrets into random online decoders.',
        },
    },
    {
        id: 'qr-code',
        name: 'QR Code',
        category: 'Utils',
        description: 'Generate QR codes',
        available: true,
        icon: QrCode,
        kind: 'tool',
        loader: () => import('$lib/tools/Utils/QrCode.svelte'),
        docs: {
            use: 'Generate QR codes for any text or URL, with PNG or SVG export.',
            offline: 'QR content can contain private links, internal URLs, or other sensitive payloads.',
            tip: 'Double-check the encoded content before sharing the QR image.',
        },
    },
    {
        id: 'format-converter',
        name: 'Format Converter',
        category: 'Utils',
        description: 'Convert & explore JSON, YAML, TOML, XML',
        available: true,
        icon: ArrowLeftRight,
        kind: 'tool',
        loader: () => import('$lib/tools/Utils/FormatConverter.svelte'),
        docs: {
            use: 'Convert or inspect structured text such as JSON/YAML/TOML/XML.',
            offline: 'Configuration files can contain API keys, internal URLs, credentials, or customer data.',
            tip: 'Remove secrets before sharing formatted output.',
        },
    },
    {
        // The recorder runs in Rust and mirrors state through a module store, so
        // it remains safe to host in the Media workspace while recording.
        id: 'screen-recorder',
        name: 'Screen Recorder',
        category: 'Media',
        description: 'Record your screen to a clean MP4 — fully on-device, nothing uploaded.',
        available: true,
        icon: Video,
        kind: 'tool',
        loader: () => import('$lib/tools/Capture/ScreenRecorder.svelte'),
    },
    {
        id: 'media-utility',
        name: 'Media Utility',
        category: 'Media',
        description: 'Extract MP3 audio or compress video for sharing',
        available: true,
        icon: Video,
        kind: 'tool',
        loader: () => import('$lib/tools/Utils/MediaUtility.svelte'),
        docs: {
            use: 'Choose a local video, then extract its audio or target a smaller video size.',
            offline: 'Media conversion runs on your device through your local FFmpeg installation.',
            tip: 'Choose a preset or enter any positive target size in MB. The tool warns before creating an unwatchable result.',
        },
    },
    {
        id: 'password-generator',
        name: 'Password Generator',
        category: 'Utils',
        description: 'Generate secure passwords and passphrases',
        available: true,
        icon: KeyRound,
        kind: 'tool',
        loader: () => import('$lib/tools/Utils/PasswordGenerator.svelte'),
        docs: {
            use: 'Generate passwords or passphrases locally.',
            offline: 'Passwords and recovery strings should not be generated by a random website you do not control.',
            tip: 'Prefer long unique passwords stored in a password manager.',
        },
    },
    // Voice to Text was previously a Utils tool; promoted to a top-bar
    // page screen because it's a first-class workflow surface that pairs
    // with the search + clipboard overlays. Registered in the pageScreens
    // block above instead of toolScreens.
    {
        id: 'color-picker',
        name: 'Color Picker',
        category: 'Utils',
        description: 'Pick a color from anywhere on screen; convert HEX/RGB/HSL/HSV',
        available: true,
        icon: Pipette,
        kind: 'tool',
        loader: () => import('$lib/tools/Utils/ColorPicker.svelte'),
        docs: {
            use: 'Pick a color from anywhere on screen with the eyedropper, or choose one manually, then copy it as HEX, RGB, HSL, or HSV.',
            offline: 'Runs entirely on-device — no color is ever sent anywhere.',
            tip: 'Recent colors are remembered locally so you can reuse a palette across sessions.',
        },
    },
    {
        id: 'util-calculator',
        name: 'Calculator',
        category: 'Utils',
        description: 'A calculating notepad — math, units, percentages and variables, all on-device.',
        available: true,
        icon: Calculator,
        kind: 'tool',
        loader: () => import('$lib/tools/Utils/UtilCalculator.svelte'),
    },
    {
        id: 'file-search',
        name: 'File Search',
        category: 'File',
        description: 'Indexed local file search with live updates',
        available: true,
        icon: FileSearch,
        kind: 'tool',
        acceptsSelected: true,
        loader: () => import('./tools/Search/FileSearch.svelte'),
        docs: {
            use: 'Build a local search index and find files using advanced search (AND, OR, quotes, ext:, path:, size:).',
            offline: 'Indexing and search stay on your machine. No file content is uploaded.',
            tip: 'Start with metadata-only indexing for huge folders, then enable content indexing when needed.',
        },
    },
    // File Manager leads the File pack deliberately (moved 2026-07-23), so a
    // category workspace opens on the safer browsing tool instead of a
    // destructive analyzer.
    {
        id: 'file-manager',
        name: 'File Manager',
        category: 'File',
        description: 'Dual-pane file manager — copy, move, rename, and back up between two folders',
        available: true,
        icon: Columns2,
        kind: 'tool',
        loader: () => import('$lib/tools/File/FileManager.svelte'),
        docs: {
            use: 'Browse two folders side by side, then copy, move, rename, delete (to Recycle Bin), make folders, or back up with robocopy (copy or mirror).',
            offline: 'Every operation runs on your machine through the OS file APIs — nothing is uploaded.',
            tip: 'Drag rows between panes to copy (hold Ctrl to move). Tab switches the active pane; Backspace goes up a folder.',
        },
    },
    {
        id: 'archive-utility',
        name: 'Archive Utility',
        category: 'File',
        description: 'Create, inspect, and extract ZIP, 7Z, and modern TAR archives locally',
        available: true,
        icon: Archive,
        kind: 'tool',
        loader: () => import('$lib/tools/File/ArchiveUtility.svelte'),
        docs: {
            use: 'Create a ZIP, 7Z, TAR.ZST, TAR.GZ, or TAR.XZ archive, or inspect and safely extract one.',
            offline: 'Archive contents are handled on your device. Nothing is uploaded.',
            tip: 'Use ZIP for compatibility, 7Z for smaller folders, and TAR.ZST for fast large backups. ZIP and 7Z can use a password.',
        },
    },
    {
        id: 'cleaner-analyzer',
        name: 'Cleaner / Analyzer',
        category: 'File',
        description: 'Analyze safe cache bloat, old large files, startup items, RAM, and disk usage',
        available: true,
        icon: Trash2,
        kind: 'tool',
        loader: () => import('$lib/tools/File/CleanerAnalyzer.svelte'),
        docs: {
            use: 'Find safe cache cleanup targets and review large old files or startup entries.',
            offline: 'Cleaner analysis stays local and does not delete personal files or applications.',
            tip: 'Only cache/temp targets can be cleaned, and cleanup requires typing CONFIRM.',
        },
    },
    {
        id: 'duplicate-finder',
        name: 'Duplicate Finder',
        category: 'File',
        description: 'Find exact duplicate files recursively and move duplicates to review',
        available: true,
        icon: FileSearch,
        kind: 'tool',
        loader: () => import('$lib/tools/File/DuplicateFileFinder.svelte'),
    },
    {
        id: 'bulk-rename',
        name: 'Bulk Rename',
        category: 'File',
        description: 'Preview and safely rename many files or folders',
        available: true,
        icon: FilePenLine,
        kind: 'tool',
        loader: () => import('$lib/tools/File/BulkRename.svelte'),
    },
    {
        id: 'diff-merge',
        name: 'Folder Diff & Merge',
        category: 'File',
        description: 'Compare two folders recursively and run 3-way text merges',
        available: true,
        icon: GitMerge,
        kind: 'tool',
        loader: () => import('$lib/tools/File/DiffMerge.svelte'),
        docs: {
            use: 'Recursively compare two folders (added/removed/modified with inline unified diffs for text files), or run a diff3 three-way merge of base/ours/theirs with conflict markers.',
            offline: 'All comparison and merging happens on-device — no files are uploaded and git is not required.',
            tip: 'Click a “modified” file to see its line-by-line diff. For merges, load each pane from a file or paste text, then resolve any conflict markers.',
        },
    },
    {
        id: 'automation-recipes',
        name: 'Automations',
        category: 'Automation',
        description: 'Build simple local recipes from KeepItLocal tools',
        available: true,
        icon: Sparkles,
        kind: 'tool',
        loader: () => import('$lib/tools/Automation/AutomationRecipes.svelte'),
    },
    {
        id: 'file-shredder',
        name: 'File Shredder',
        category: 'Privacy',
        description: 'Securely delete files',
        available: true,
        icon: Trash2,
        kind: 'tool',
        loader: () => import('$lib/tools/Privacy/FileShredder.svelte'),
        docs: {
            use: 'Delete selected files more aggressively than normal deletion.',
            offline: 'Destructive file operations should stay under your control and should never be delegated to a website.',
            tip: 'Use carefully and keep backups of important files. SSD behavior can differ from old hard drives.',
        },
    },
    {
        id: 'screenshot-redact',
        name: 'Screenshot Redact',
        category: 'Privacy',
        description: 'Blur or black out sensitive areas',
        available: true,
        icon: EyeOff,
        kind: 'tool',
        loader: () => import('$lib/tools/Privacy/ScreenshotRedact.svelte'),
        docs: {
            use: 'Hide sensitive regions in screenshots.',
            offline: 'Screenshots often contain names, tokens, emails, addresses, customer data, tabs, or account details.',
            tip: 'Prefer solid redaction for secrets. Blur can sometimes leak shape/context.',
        },
    },
    {
        id: 'windows-hardening',
        name: 'Privacy Hardening',
        category: 'Privacy',
        description: 'Reversible Windows privacy tweaks — no admin needed',
        available: true,
        // Hidden 2026-07-23. The ship-readiness audit found this is the one tool
        // that genuinely LOSES to its free competitor rather than tying: ~13
        // settings vs O&O ShutUp10 / W10Privacy's 50-100+. Shipping it at parity
        // with the others sets an expectation it can't meet. Un-hide either after
        // broadening coverage, or with explicit "curated safe subset, not a
        // ShutUp10 replacement" framing.
        hidden: true,
        icon: ShieldCheck,
        kind: 'tool',
        loader: () => import('$lib/tools/Privacy/WindowsHardening.svelte'),
        docs: {
            use: 'Turn off Windows telemetry, ad tracking, and suggested-content ads with reversible one-click toggles. Admin-only tweaks come with a step-by-step guide.',
            offline: 'These are your own account settings — changed locally, never sent anywhere, and every change can be undone.',
            tip: 'Everything here is user-scope and needs no admin. Use Export backup before a big change, and Revert any time.',
        },
    },
    {
        // Unified Word converter — replaces the three separate Word→PDF /
        // Word→Markdown / Word→Text entries that previously cluttered the
        // sidebar. The merged screen has a target-format selector and
        // dispatches to the appropriate underlying conversion store.
        id: 'word-converter',
        name: 'Word Converter',
        category: 'Document',
        description: 'Convert .docx to clean Markdown or plain text',
        available: true,
        // Hidden 2026-07-23: superseded in the Documents pack by the Markdown
        // Converter (docx -> Markdown is the same trip in reverse, and the new
        // tool owns the conversion story). Code + Rust commands stay registered.
        hidden: true,
        icon: FileText,
        kind: 'tool',
        loader: () => import('$lib/tools/Document/WordConverter.svelte'),
        docs: {
            use: 'Convert Word .docx files to GitHub-Flavored Markdown (headings, bold/italic, links, lists, tables, images) or plain text. All processing is local.',
            offline: 'Documents often contain personal, legal, business, or client information. Conversion never uploads anything.',
            tip: 'Great for getting Word content into notes, git repos, or static sites. For PDF, use Word\'s built-in "Save as PDF".',
        },
    },
    {
        // Markdown Converter — absorbed the devkit Markdown panel (2026-07-23)
        // and gained real file export. Leads the Documents pack because it is
        // now the pack's broadest tool (four output formats) and replaces the
        // hidden Word Converter as the conversion story.
        id: 'markdown-converter',
        name: 'Markdown Converter',
        category: 'Document',
        description: 'Preview Markdown and export it to HTML, PDF, Word, or plain text',
        available: true,
        icon: FileText,
        kind: 'tool',
        loader: () => import('$lib/tools/Document/MarkdownConverter.svelte'),
        docs: {
            use: 'Write or paste Markdown (or open a .md file), preview it live, then export to HTML, PDF, Word (.docx), or plain text. Also converts HTML back into clean Markdown.',
            offline: 'Rendering, conversion and export all run on your machine — nothing is uploaded.',
            tip: 'PDF and Word share the same renderer as the Notes export, so headings, tables and lists come out styled rather than as flat text.',
        },
    },
    {
        id: 'csv-toolkit',
        name: 'CSV Toolkit',
        category: 'Document',
        description: 'Convert Excel ⇄ CSV, merge, clean, split, and export CSV to JSON',
        available: true,
        icon: FileSpreadsheet,
        kind: 'tool',
        loader: () => import('$lib/tools/Document/CsvToolkit.svelte'),
        docs: {
            use: 'Every CSV / spreadsheet job in one tool: convert spreadsheets to and from Excel (XLSX/XLS/XLSB/ODS), merge many CSV/TSV files into one, clean messy exports, split big files into chunks, and export CSV to JSON / JSONL.',
            offline: 'Spreadsheets and CSV exports frequently hold customer lists, prices, finances, CRM data, and other private operational data; every mode processes files on-device with no upload.',
            tip: 'Pick a mode from the left rail: Convert round-trips Excel ⇄ CSV, Merge appends files, Clean normalizes messy exports, Split chunks big files, and → JSON converts to JSONL / JSON.',
        },
    },
    {
        id: 'doc-password',
        name: 'Remove Password',
        category: 'Document',
        description: 'Remove the "restrict editing" lock from Word .docx files',
        available: true,
        // Hidden 2026-07-23 (owner call). Narrow single-purpose tool; the PDF
        // half of this story already moved to KeepItLocal Privacy. Code stays.
        hidden: true,
        icon: Lock,
        kind: 'tool',
        loader: () => import('./tools/Document/RemovePassword.svelte'),
        docs: {
            use: 'Strip the "restrict editing" XML flag from a Word .docx file (no password needed — that protection is non-cryptographic).',
            offline: 'All processing is local; the original file is preserved and an unlocked copy is written alongside it.',
            tip: 'Word\'s "encrypt with open password" (full AES encryption) isn\'t supported — open the doc in Word and save without password to remove that variant. (PDF unlocking now lives in KeepItLocal Privacy.)',
        },
    },
    {
        id: 'dev-toolkit',
        name: 'Developer Tools',
        category: 'Development',
        description: 'JWT, regex, SQL, diff, Markdown, ID/data generators, cron, and secret scanning in one hub',
        available: true,
        icon: Wrench,
        kind: 'tool',
        loader: () => import('$lib/tools/Development/DevToolkit.svelte'),
        docs: {
            use: 'A local-first developer toolbox grouped into Format & Inspect (JWT decoder, regex, SQL formatter, diff, Markdown), Generate (IDs, fake data, cron), and Secure (secret scanner). Pick a tool from the left rail.',
            offline: 'Tokens, queries, snippets, and logs often carry secrets, IDs, or production data. Every tool runs on-device with no upload.',
            tip: 'SSH Key Manager and Encrypt / Decrypt stay separate tools — they manage on-disk key material and files.',
        },
    },
    {
        id: 'ssh-key-manager',
        name: 'SSH Key Manager',
        category: 'Development',
        description: 'Generate, import, and manage SSH keys, config hosts, and known_hosts',
        available: true,
        icon: KeySquare,
        kind: 'tool',
        loader: () => import('$lib/tools/Development/SshKeyManager.svelte'),
        docs: {
            use: 'Generate Ed25519/RSA/ECDSA keys (optionally passphrase-encrypted), import existing keys, copy public keys, edit ~/.ssh/config host entries, and clean up known_hosts.',
            offline: 'Everything runs on your machine via a pure-Rust SSH library. Private key material is never shown in the UI or sent anywhere.',
            tip: 'Ed25519 is the recommended default — fast, small, and modern. Add a passphrase for keys that protect important servers.',
        },
    },
    {
        id: 'encrypt-decrypt',
        name: 'Encrypt / Decrypt',
        category: 'Development',
        description: 'Password-encrypt files and text with AES-256-GCM + Argon2id',
        available: true,
        icon: FileLock2,
        kind: 'tool',
        loader: () => import('$lib/tools/Development/EncryptDecrypt.svelte'),
        docs: {
            use: 'Encrypt or decrypt any file (streamed, batch) or a text message with a password — optionally combined with a keyfile. Output lands next to the source (.kenc).',
            offline: 'Authenticated AES-256-GCM with an Argon2id-derived key, all on-device. A wrong password/keyfile or any tampering is detected and fails loudly.',
            tip: 'Use a long passphrase. The keyfile option adds a second factor — keep it safe and unchanged, or the data becomes unrecoverable.',
        },
    },
    {
        id: 'image-studio',
        name: 'Image Studio',
        category: 'Image',
        description: 'Resize, crop, compress, convert, and remove image backgrounds locally',
        available: true,
        icon: ImageIcon,
        kind: 'tool',
        loader: () => import('$lib/tools/Image/ImageStudio.svelte'),
        docs: {
            use: 'Resize, crop, compress, convert, or remove the background from one image; batch mode handles resize, conversion, and compression.',
            offline: 'Photos and screenshots often hold private scenes, people, locations, or work assets. Every operation runs on-device — nothing is uploaded.',
            tip: 'Use Single mode for crop and background removal. Fast removal works immediately; High quality is an optional model download in Settings.',
        },
    },
    {
        id: 'ocr-image-to-text',
        name: 'Image to Text (OCR)',
        category: 'Image',
        description: 'Extract text from images on-device with Tesseract — English, Georgian, and more',
        available: true,
        icon: ScanText,
        kind: 'tool',
        loader: () => import('$lib/tools/Image/OcrTool.svelte'),
        docs: {
            use: 'Pick or drop an image, choose a language, and extract its text on-device with Tesseract.',
            offline: 'Scans and photos often hold IDs, contracts, receipts, or personal documents. OCR runs entirely on your machine — nothing is uploaded.',
            tip: 'Higher-resolution scans give much better results. English, Georgian, and Russian models are bundled — no separate install needed.',
        },
    },
    {
        id: 'camscanner',
        name: 'Document Scanner',
        category: 'Image',
        description: 'Turn a photo of a document into a clean, flattened scan',
        available: true,
        icon: ScanLine,
        kind: 'tool',
        loader: () => import('$lib/tools/Image/CamScanner.svelte'),
        docs: {
            use: 'Import or paste a photo of a document, drag the four corners to fit the page, pick an enhancement (Color, Grayscale, B&W, or Magic), then export the flattened scan or extract its text.',
            offline: 'Contracts, IDs, receipts, and personal paperwork stay on your machine. Corner detection, perspective-flattening, enhancement, and OCR all run on-device — nothing is uploaded.',
            tip: 'Photograph the document on a contrasting surface in even light so the page edges are easy to detect. Magic gives the crispest scanned-paper look; B&W is best before extracting text.',
        },
    },
    {
        id: 'img-base64',
        name: 'Image to Base64',
        // Moved Image -> Development (2026-07-23). A Base64 data URI is a
        // developer artifact (inlining an asset into CSS/HTML/JSON), not photo
        // editing — it sat oddly beside Studio/OCR/Watermark. `toolPackIdForScreen`
        // derives the pack from this field, so the category change is the move.
        category: 'Development',
        description: 'Convert images to Base64 data URIs',
        available: true,
        icon: ImageIcon,
        kind: 'tool',
        loader: () => import('$lib/tools/Image/ImgBase64.svelte'),
    },
    {
        id: 'img-favicon',
        name: 'Favicon Generator',
        category: 'Image',
        description: 'Create favicon packs from one image',
        available: true,
        icon: ImageIcon,
        kind: 'tool',
        loader: () => import('$lib/tools/Image/FaviconGenerator.svelte'),
        docs: {
            use: 'Generate favicon and app icon files from one source image.',
            offline: 'Brand assets and unpublished logos can stay on your machine.',
            tip: 'Start with a square, high-resolution PNG for cleaner output.',
        },
    },
    {
        id: 'img-watermark',
        name: 'Watermark',
        category: 'Image',
        description: 'Add text or logo watermarks to images',
        available: true,
        icon: ImageIcon,
        kind: 'tool',
        loader: () => import('$lib/tools/Image/ImageWatermark.svelte'),
        docs: {
            use: 'Add text or logo watermarks to one or many images.',
            offline: 'Watermarking often involves original assets that should remain local until you choose to publish them.',
            tip: 'Use per-file settings when some images need different positions or opacity.',
        },
    },
];

export const tools = toolScreens;

/* ─── Category Workspace screens ───────────────────────────────────
   Each pack (excluding core) that ships ≥2 tools gets ONE page screen
   `category-<packId>` that renders CategoryWorkspace — compact pack context
   plus the active tool. Single-tool packs (e.g.
   Automation) keep their tool as a direct screen. These are page
   screens in the full catalog; `screensForPacks` installs only the
   workspaces whose packs are enabled. */
const WORKSPACE_MIN_TOOLS = 2;

function toolsForPackId(packId: ToolPackId): ToolScreen[] {
    return toolScreens.filter((screen) => toolPackIdForScreen(screen) === packId);
}

export const categoryWorkspacePackIds: ToolPackId[] = toolPacks
    .filter((pack) => pack.id !== 'core' && toolsForPackId(pack.id).length >= WORKSPACE_MIN_TOOLS)
    .map((pack) => pack.id);

export const categoryScreens: PageScreen[] = categoryWorkspacePackIds.map((packId) => ({
    id: `category-${packId}`,
    name: packLabels[packId],
    description: toolPacks.find((pack) => pack.id === packId)?.description ?? '',
    kind: 'page',
    loader: () => import('$lib/CategoryWorkspace.svelte'),
    acceptsSelected: true,
}));

/** Map a tool id → its Category Workspace screen id, or null when the
 *  tool has no workspace (core tools, or a single-tool pack). The
 *  router uses this to redirect tool navigations into the workspace. */
export function categoryScreenIdForTool(toolId: string): string | null {
    if (typeof toolId !== 'string') return null;
    const screen = toolScreens.find((tool) => tool.id === toolId);
    if (!screen) return null;
    const packId = toolPackIdForScreen(screen);
    return categoryWorkspacePackIds.includes(packId) ? `category-${packId}` : null;
}

/** packId for a `category-<packId>` screen id (or null). */
export function packIdForCategoryScreen(screenId: string): ToolPackId | null {
    if (typeof screenId !== 'string' || !screenId.startsWith('category-')) return null;
    const packId = screenId.slice('category-'.length) as ToolPackId;
    return categoryWorkspacePackIds.includes(packId) ? packId : null;
}

export const allScreens = [...pageScreens, ...categoryScreens, ...toolScreens];

export const documentationScreens = toolScreens.filter((screen) => Boolean(screen.docs));

export const aboutQuickStarts: QuickStartCard[] = [
    { label: 'Protect a file before sharing', targetId: 'privacy-guide', hint: 'Learn what metadata and account risks to check first.' },
    { label: 'Learn how tools work', targetId: 'documentation', hint: 'Open the KeepItLocal documentation and usage guide.' },
    { label: 'Resize, convert, or compress images', targetId: 'image-studio', hint: 'Resize, convert, and compress photos locally — no uploads.' },
];

export const builtInProfileDefinitions: BuiltInProfileDefinition[] = [
    {
        id: 'all',
        name: 'All Tools',
        description: 'Show every tool',
        includeAll: true,
    },
    {
        id: 'privacy',
        name: 'Privacy-Focused',
        description: 'Tools for protecting and cleaning data',
        includeToolIds: ['file-shredder', 'screenshot-redact', 'hash-check', 'encoders'],
    },
    {
        id: 'office',
        name: 'Office',
        description: 'Documents and spreadsheets',
        includeToolIds: [
            'csv-toolkit',
            'word-converter',
            'qr-code',
            'hash-check',
            'image-studio',
        ],
    },
    {
        id: 'developer',
        name: 'Developer',
        description: 'Dev utilities and data tools',
        includeCategories: ['Utils', 'Development'],
    },
    {
        id: 'lite',
        name: 'Lite',
        description: 'Just the essentials',
        includeToolIds: ['hash-check', 'encoders', 'qr-code', 'format-converter'],
    },
];

const screenMap = new Map(allScreens.map((screen) => [screen.id, screen]));

export function toolPackIdForScreen(screen: ToolScreen): ToolPackId {
    if (screen.packId) return screen.packId;
    if (screen.id === 'file-search') return 'core';
    const categoryToPack: Record<Category, ToolPackId> = {
        Utils: 'utils',
        Development: 'development',
        Privacy: 'privacy',
        Document: 'document',
        Image: 'image',
        Media: 'media',
        File: 'file',
        Automation: 'automation',
        Focus: 'time-focus',
    };
    return categoryToPack[screen.category];
}

export function toolScreensForPacks(enabledPackIds: string[]) {
    const enabled = new Set(enabledPackIds);
    enabled.add('core');
    return toolScreens.filter((screen) => enabled.has(toolPackIdForScreen(screen)));
}

export function screensForPacks(enabledPackIds: string[]) {
    const enabled = new Set(enabledPackIds);
    enabled.add('core');
    return [
        ...pageScreens,
        ...categoryScreens.filter((screen) => {
            const packId = packIdForCategoryScreen(screen.id);
            return packId === null || enabled.has(packId);
        }),
        ...toolScreensForPacks(enabledPackIds),
    ];
}

export function getScreen(id: string) {
    return screenMap.get(id);
}

export function hasScreen(id: string) {
    return screenMap.has(id);
}

export function toolIdsByList(ids: string[]) {
    return toolScreens.filter((screen) => ids.includes(screen.id)).map((screen) => screen.id);
}

export function toolIdsByCategory(categories: Category[]) {
    return toolScreens
        // Hidden tools are excluded at the source, which is correct for every
        // caller: built-in profiles must not select a tool the user can't see,
        // and the Welcome pack counts must match what actually shows up.
        .filter((screen) => categories.includes(screen.category) && !screen.hidden)
        .map((screen) => screen.id);
}

export type Tool = ToolScreen;
