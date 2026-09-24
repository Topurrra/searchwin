/*
  The KeepItLocal component kit — one canonical, macOS-grade Svelte 5
  kit, built to DESIGN.md. Every screen composes from here; new
  surfaces import from `$lib/ui` and never re-roll markup.
*/
export { default as Button } from './Button.svelte';
export { default as Card } from './Card.svelte';
export { default as SectionHeader } from './SectionHeader.svelte';
export { default as TextInput } from './TextInput.svelte';
export { default as Textarea } from './Textarea.svelte';
export { default as Select } from './Select.svelte';
export { default as Toggle } from './Toggle.svelte';
export { default as Checkbox } from './Checkbox.svelte';
export { default as Tabs } from './Tabs.svelte';
export { default as Kbd } from './Kbd.svelte';
export { default as ListRow } from './ListRow.svelte';
export { default as Pager } from './Pager.svelte';

/*
  UI/UX upgrade track — Phase 1 primitives. The migration target for
  every tool / pillar / setting surface. Composes from the existing
  primitives above; never duplicates them.
*/
export { default as ToolPage } from './ToolPage.svelte';
export { default as ToolToolbar } from './ToolToolbar.svelte';
export { default as ToolPanel } from './ToolPanel.svelte';
export { default as ResultList } from './ResultList.svelte';
export { default as ResultRow } from './ResultRow.svelte';
export { default as ErrorState } from './ErrorState.svelte';
export { default as SideSheet } from './SideSheet.svelte';
export { default as CollapsibleCard } from './CollapsibleCard.svelte';

/*
  Existing kit members. Re-exported so `$lib/ui` is the single import
  surface; the files physically move into this folder in the Phase 6
  polish pass.
*/
export { default as EmptyState } from '../components/EmptyState.svelte';
export { default as LoadingState } from '../components/LoadingState.svelte';
