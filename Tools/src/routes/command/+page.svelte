<script lang="ts">
    /* The command palette route, mounted by the global-hotkey popup window
     * (label 'command', see lib.rs get_or_create_command_window).
     *
     * History: this file used to BE the palette (V1, ~14k lines). V2 shipped
     * beside it at a temporary /command-v2 route while V1 stayed as the
     * rollback path; once V2 had real mileage, V1 was deleted (2026-07-18) and
     * the canonical /command URL was handed to V2. V1 remains recoverable from
     * git history at f521f72 if it is ever needed again.
     *
     * PaletteV2 takes no props here: both are optional, and `asOverlay` must
     * stay false so it detects the real (transparent) command window rather
     * than the embedded-in-main overlay. */
    import PaletteV2 from './PaletteV2.svelte';
</script>

<PaletteV2 />
