/*
  commandPalette — controls the embedded command-palette overlay.

  TEST BINDING (Phase 3.6.5): the new `/command` palette is rendered as
  an OVERLAY layered on top of the live workspace (the main route stays
  mounted behind it) rather than navigating the webview to the `/command`
  route — that way the workspace stays visible behind the palette, like a
  real Spotlight/Raycast overlay floats over the desktop.

  Any component can open it (`commandPaletteOpen.set(true)`); the main
  route (`src/routes/+page.svelte`) watches this and conditionally mounts
  `<PaletteV2 asOverlay … />` over everything else.

  In production (Phase 3.6.6) the palette gets its own transparent Tauri
  overlay window and this store is retired — the window itself provides
  the "floats over the desktop" effect at the OS level.
*/

import { writable } from 'svelte/store';

export const commandPaletteOpen = writable(false);

export function openCommandPalette() {
    commandPaletteOpen.set(true);
}

export function closeCommandPalette() {
    commandPaletteOpen.set(false);
}
