/** Workspace screens Search leaves out, and why: the one list.
 *
 *  A screen named here is not in the tools index, not offered in the field,
 *  and not in Settings › Packs; its address says why instead of opening it.
 *  Its code stays until it's deleted for good, so bringing one back is
 *  taking its line out. The reason is shown to the person who asked. */
export const offInSearch: Readonly<Record<string, string>> = {
    // Later phases.
    'word-converter': 'Word ⇄ PDF comes to Search in a later version.',
    snippets: 'Snippets come to Search with its background mode, in a later version.',
    'voice-to-text': 'Voice comes to Search in a later version.',
    'automation-recipes': 'Recipes come to Search with its agent, in a later version.',
    'screen-recorder': 'Screen Recorder needs native Search capture controls before it can be used here.',
    reminders: 'Reminders need Search background scheduling before they can fire reliably.',

    // Search does these itself.
    'clipboard-history': 'Search keeps its own clipboard history: press Ctrl+Shift+V.',
    'my-commands': 'Search does this in the field: bangs (!yt cats) and commands (>new tab).',
    profiles: 'Search keeps different sets of tabs in spaces.',
    settings: 'Search has its own Settings: press Ctrl+,.',
    'privacy-guide': "Search's privacy is in Settings › Privacy.",
    about: 'About Search is in its menu.',
    home: 'Search opens a new tab for this.',
    pinned: 'Search pins tabs instead.',
    command: 'Search does this in the field: press Ctrl+L.',
    notes: 'Notes come to Search as a side panel, in a later version.',

    // Not better than the free ones yet (Workspace's own call).
    'windows-hardening': 'Privacy Hardening is waiting until it covers more than the free tools do.',
    'doc-password': 'Remove Password is waiting for a wider use than one Word setting.',
};
