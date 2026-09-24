/*
  settingsTarget — deep-link channel for the Settings page.

  Anywhere in the app can ask the Settings page to open with a specific
  section pre-selected by setting this store BEFORE switching `selected`
  to 'settings'. The Settings component reads + clears this on mount so
  it's a one-shot signal, not a persistent preference.

  Use cases:
    - Sidebar / Tools page click on an inactive tool → land the user on
      Settings → Tool Packs so they can enable the pack and restart.
    - Empty-state CTA on a broken tool page → "Configure in Settings"
      that drops the user directly on the right section.

  Why a store rather than a query param: the router doesn't use URL
  query strings (it's a SPA route-by-state). A writable store is the
  lightest cross-component handoff.
*/

import { writable } from 'svelte/store';

/** Valid section IDs in src/lib/tools/TopBar/Settings.svelte. */
export type SettingsSectionId =
    | 'system'
    | 'appearance'
    | 'shortcuts'
    | 'searchOverlay'
    | 'clipboard'
    | 'voice'
    | 'imageModels'
    | 'fileIndex'
    | 'contentIndex'
    | 'indexing'
    | 'toolpacks'
    | 'onboarding'
    | 'storage'
    | 'logs'
    | 'note'
    | 'reset';

/** Non-null means: when Settings mounts next, jump straight to this
 *  section. The Settings component clears the store after reading. */
export const settingsTarget = writable<SettingsSectionId | null>(null);

/** Convenience helper for the common pattern: "navigate to Settings
 *  with a specific section pre-selected." Callers are responsible for
 *  setting `selected = 'settings'` (this store doesn't drive routing). */
export function requestSettingsSection(section: SettingsSectionId) {
    settingsTarget.set(section);
}
