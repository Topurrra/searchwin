<script lang="ts">
    // Search's own media player: https://tools.search/#/play?path=<encoded>.
    // Reached from the field's Play action (Browser.Field.cs → ToolsHost) for
    // a video or audio file, never typed at a web page — files.search only
    // serves tool pages (see ToolsHost.cs), and this page only ever asks it
    // for the path the browser handed it or a sibling the engine listed.
    import { invoke, convertFileSrc } from '@tauri-apps/api/core';
    import {
        Play,
        Pause,
        SkipBack,
        SkipForward,
        Volume2,
        VolumeX,
        Maximize,
        Minimize,
        ListMusic,
        Music,
    } from '@lucide/svelte';

    // What WebView2 (Chromium) actually plays without the FFmpeg add-on.
    // Kept in sync by hand with SearchKit.Field.FileKinds.Playable — see
    // Search.Kit/Field/FieldRow.cs and its tests.
    const VIDEO_EXT = new Set(['mp4', 'm4v', 'webm', 'mov', 'ogv']);
    const AUDIO_EXT = new Set(['mp3', 'm4a', 'aac', 'wav', 'ogg', 'oga', 'opus', 'flac']);

    type Kind = 'video' | 'audio' | 'unsupported';

    function extOf(path: string): string {
        const m = /\.([^.\\/]+)$/.exec(path);
        return m ? m[1].toLowerCase() : '';
    }
    function kindOf(path: string): Kind {
        const ext = extOf(path);
        if (VIDEO_EXT.has(ext)) return 'video';
        if (AUDIO_EXT.has(ext)) return 'audio';
        return 'unsupported';
    }
    function baseName(path: string): string {
        const m = /[^\\/]+$/.exec(path);
        return m ? m[0] : path;
    }
    function folderOf(path: string): string {
        const i = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
        return i >= 0 ? path.slice(0, i) : '';
    }
    function sameFile(a: string, b: string): boolean {
        return a.replace(/\//g, '\\').toLowerCase() === b.replace(/\//g, '\\').toLowerCase();
    }

    interface FolderChild {
        name: string;
        isDir: boolean;
        size: number;
        modifiedMs: number;
    }
    interface Track {
        name: string;
        path: string;
        kind: Kind;
    }

    // SvelteKit's hash router matches the ROUTE (`/play`) from the hash, but
    // `page.url` keeps the real (empty) query string — the `?path=` lives
    // inside the fragment, which `page.url.searchParams` never sees. So the
    // query is read by hand, from `location.hash` itself, kept in sync with
    // `popstate` (the tab's own Back/Forward) and `hashchange`.
    function readPath(): string {
        try {
            const hash = location.hash.startsWith('#') ? location.hash.slice(1) : location.hash;
            return new URL(hash, location.origin).searchParams.get('path') ?? '';
        } catch {
            return '';
        }
    }
    let path = $state(readPath());
    $effect(() => {
        const sync = () => (path = readPath());
        window.addEventListener('popstate', sync);
        window.addEventListener('hashchange', sync);
        return () => {
            window.removeEventListener('popstate', sync);
            window.removeEventListener('hashchange', sync);
        };
    });

    const kind = $derived(kindOf(path));
    const name = $derived(path ? baseName(path) : '');
    const folder = $derived(path ? folderOf(path) : '');
    const src = $derived(path ? convertFileSrc(path) : '');

    let media = $state<HTMLVideoElement | HTMLAudioElement | null>(null);
    let stage = $state<HTMLDivElement | null>(null);
    let playing = $state(false);
    let muted = $state(false);
    let volume = $state(1);
    let current = $state(0);
    let duration = $state(0);
    let fullscreen = $state(false);
    let mediaError = $state(false);
    let showList = $state(false);

    let tracks = $state<Track[]>([]);
    let tracksFolder = $state<string | null>(null);

    // The playlist is the folder's other media, fetched once per folder (not
    // once per file) so next/previous inside the same folder is instant.
    $effect(() => {
        const wanted = folder;
        if (!wanted || wanted === tracksFolder) return;
        tracksFolder = wanted;
        invoke<FolderChild[]>('list_folder_children', { path: wanted })
            .then((children) => {
                if (folder !== wanted) return; // moved on while this was in flight
                tracks = children
                    .filter((c) => !c.isDir)
                    .map((c) => ({ name: c.name, path: `${wanted}\\${c.name}`, kind: kindOf(c.name) }))
                    .filter((t) => t.kind !== 'unsupported');
            })
            .catch(() => {
                if (folder === wanted) tracks = [];
            });
    });

    const index = $derived(tracks.findIndex((t) => sameFile(t.path, path)));

    // A real history entry (so the tab's Back goes to the previous track, and
    // the tab's address — what bench and a bookmark would see — names the
    // file actually playing), pushed by hand rather than through `goto`:
    // SvelteKit's hash router only matches the route from the hash, and
    // drops a `?path=` living inside it on the way back out.
    function go(newPath: string) {
        history.pushState(history.state, '', `#/play?path=${encodeURIComponent(newPath)}`);
        path = newPath;
    }
    function next() {
        if (tracks.length === 0) return;
        const at = index < 0 ? -1 : index;
        go(tracks[(at + 1) % tracks.length].path);
    }
    function previous() {
        if (tracks.length === 0) return;
        const at = index < 0 ? 0 : index;
        go(tracks[(at - 1 + tracks.length) % tracks.length].path);
    }

    // MARK: - remembered position (localStorage on tools.search)

    function storageKey(p: string): string {
        return `search-player:position:${p.replace(/\//g, '\\').toLowerCase()}`;
    }
    function loadPosition(p: string): number {
        try {
            const raw = localStorage.getItem(storageKey(p));
            const value = raw ? Number(raw) : 0;
            return Number.isFinite(value) && value > 0 ? value : 0;
        } catch {
            return 0;
        }
    }
    function savePosition(p: string, time: number) {
        try {
            localStorage.setItem(storageKey(p), String(Math.floor(time)));
        } catch {
            /* private mode, quota, or storage disabled — just don't remember */
        }
    }

    let lastSaved = 0;
    function onTimeUpdate() {
        if (!media) return;
        current = media.currentTime;
        // Once every couple of seconds, not every frame.
        if (current - lastSaved >= 2) {
            lastSaved = current;
            savePosition(path, current);
        }
        setPositionState();
    }
    function onLoadedMetadata() {
        if (!media) return;
        duration = media.duration || 0;
        // A file remembered within its last 5 seconds starts over, not right
        // back at the end.
        const saved = loadPosition(path);
        if (saved > 0 && saved < duration - 5) media.currentTime = saved;
        setMetadata();
        setPositionState();
        void media.play().catch(() => {
            playing = false;
        });
    }
    function onEnded() {
        savePosition(path, 0);
        if (tracks.length > 0) next();
    }
    function onPause() {
        playing = false;
        if (media) savePosition(path, media.currentTime);
    }

    // MARK: - MediaSession: Windows' media overlay and the hardware keys

    function setMetadata() {
        if (!('mediaSession' in navigator)) return;
        try {
            navigator.mediaSession.metadata = new MediaMetadata({
                title: name,
                artist: baseName(folder) || 'Search',
                album: 'Search',
            });
        } catch {
            /* MediaMetadata not available in this WebView2 build */
        }
    }
    function setPositionState() {
        if (!('mediaSession' in navigator) || !media || !duration || !Number.isFinite(duration)) return;
        try {
            navigator.mediaSession.setPositionState({
                duration,
                playbackRate: media.playbackRate || 1,
                position: Math.min(media.currentTime, duration),
            });
        } catch {
            /* position state not supported */
        }
    }
    function setActionHandlers() {
        if (!('mediaSession' in navigator)) return;
        const set = (action: MediaSessionAction, handler: MediaSessionActionHandler | null) => {
            try {
                navigator.mediaSession.setActionHandler(action, handler);
            } catch {
                /* an action this WebView2 build doesn't know */
            }
        };
        set('play', () => togglePlay());
        set('pause', () => togglePlay());
        set('previoustrack', () => previous());
        set('nexttrack', () => next());
        set('seekbackward', () => seek(-10));
        set('seekforward', () => seek(10));
        set('seekto', (details) => {
            if (media && details.seekTime != null) media.currentTime = details.seekTime;
        });
        set('stop', () => {
            if (media) media.pause();
        });
    }
    // Set once: the handlers close over `media` (via the `bind:this` binding,
    // read fresh on every call) and the navigation functions, none of which
    // are ever replaced, so there's nothing to tear down or re-register.
    $effect(() => setActionHandlers());

    // MARK: - controls

    function togglePlay() {
        if (!media) return;
        if (media.paused) void media.play().catch(() => {});
        else media.pause();
    }
    function seek(delta: number) {
        if (!media || !duration) return;
        media.currentTime = Math.max(0, Math.min(duration, media.currentTime + delta));
    }
    function setVolume(next: number) {
        volume = Math.max(0, Math.min(1, next));
        if (volume > 0) muted = false;
        if (media) {
            media.volume = volume;
            media.muted = muted;
        }
    }
    function toggleMute() {
        muted = !muted;
        if (media) media.muted = muted;
    }
    async function toggleFullscreen() {
        if (!stage) return;
        if (document.fullscreenElement) await document.exitFullscreen().catch(() => {});
        else await stage.requestFullscreen().catch(() => {});
    }
    function fmt(seconds: number): string {
        if (!Number.isFinite(seconds) || seconds < 0) return '0:00';
        const total = Math.floor(seconds);
        const h = Math.floor(total / 3600);
        const m = Math.floor((total % 3600) / 60);
        const s = total % 60;
        const mm = h > 0 ? String(m).padStart(2, '0') : String(m);
        return h > 0 ? `${h}:${mm}:${String(s).padStart(2, '0')}` : `${mm}:${String(s).padStart(2, '0')}`;
    }
    function onScrub(e: Event) {
        if (!media) return;
        media.currentTime = Number((e.target as HTMLInputElement).value);
    }

    $effect(() => {
        document.title = name || 'Play';
    });

    // Loading a new path resets the per-file UI state; the effects above
    // (loadedmetadata) pick the remembered position back up.
    $effect(() => {
        void path;
        mediaError = false;
        playing = false;
        current = 0;
        duration = 0;
        lastSaved = 0;
    });

    function onKeydown(e: KeyboardEvent) {
        if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return;
        const target = e.target as HTMLElement | null;
        if (target && /^(INPUT|TEXTAREA)$/.test(target.tagName)) return;
        switch (e.key) {
            case ' ':
                e.preventDefault();
                togglePlay();
                break;
            case 'ArrowLeft':
                e.preventDefault();
                seek(-5);
                break;
            case 'ArrowRight':
                e.preventDefault();
                seek(5);
                break;
            case 'ArrowUp':
                e.preventDefault();
                setVolume(volume + 0.1);
                break;
            case 'ArrowDown':
                e.preventDefault();
                setVolume(volume - 0.1);
                break;
            case 'f':
            case 'F':
                void toggleFullscreen();
                break;
            case 'm':
            case 'M':
                toggleMute();
                break;
            case 'n':
            case 'N':
                next();
                break;
            case 'p':
            case 'P':
                previous();
                break;
        }
    }

    $effect(() => {
        const onFsChange = () => (fullscreen = document.fullscreenElement === stage);
        document.addEventListener('fullscreenchange', onFsChange);
        return () => document.removeEventListener('fullscreenchange', onFsChange);
    });
</script>

<svelte:window onkeydown={onKeydown} />

<div class="player" class:audio={kind === 'audio'}>
    {#if !path}
        <p class="empty">No file to play.</p>
    {:else if kind === 'unsupported'}
        <div class="unsupported">
            <Music size={28} />
            <p>“{name}” needs the FFmpeg add-on — coming later.</p>
        </div>
    {:else}
        <div class="stage" bind:this={stage} class:fullscreen>
            {#if kind === 'video'}
                <!-- svelte-ignore a11y_media_has_caption -->
                <video
                    bind:this={media}
                    {src}
                    ontimeupdate={onTimeUpdate}
                    onloadedmetadata={onLoadedMetadata}
                    onended={onEnded}
                    onplay={() => (playing = true)}
                    onpause={onPause}
                    onerror={() => (mediaError = true)}
                    onclick={togglePlay}
                    ondblclick={toggleFullscreen}
                ></video>
            {:else}
                <div class="audio-face">
                    <Music size={48} />
                    <p class="audio-name">{name}</p>
                </div>
                <audio
                    bind:this={media}
                    {src}
                    ontimeupdate={onTimeUpdate}
                    onloadedmetadata={onLoadedMetadata}
                    onended={onEnded}
                    onplay={() => (playing = true)}
                    onpause={onPause}
                    onerror={() => (mediaError = true)}
                ></audio>
            {/if}

            {#if mediaError}
                <div class="error-overlay">
                    <p>“{name}” needs the FFmpeg add-on — coming later.</p>
                </div>
            {/if}

            <div class="controls">
                <button class="icon" onclick={previous} disabled={tracks.length < 2} title="Previous (p)">
                    <SkipBack size={18} />
                </button>
                <button class="icon primary" onclick={togglePlay} title="Play/Pause (space)">
                    {#if playing}<Pause size={20} />{:else}<Play size={20} />{/if}
                </button>
                <button class="icon" onclick={next} disabled={tracks.length < 2} title="Next (n)">
                    <SkipForward size={18} />
                </button>

                <span class="time">{fmt(current)}</span>
                <input
                    class="scrub"
                    type="range"
                    min="0"
                    max={duration || 0}
                    step="0.1"
                    value={current}
                    oninput={onScrub}
                />
                <span class="time">{fmt(duration)}</span>

                <button class="icon" onclick={toggleMute} title="Mute (m)">
                    {#if muted || volume === 0}<VolumeX size={18} />{:else}<Volume2 size={18} />{/if}
                </button>
                <input
                    class="volume"
                    type="range"
                    min="0"
                    max="1"
                    step="0.05"
                    value={muted ? 0 : volume}
                    oninput={(e) => setVolume(Number((e.target as HTMLInputElement).value))}
                />

                {#if tracks.length > 1}
                    <button class="icon" onclick={() => (showList = !showList)} title="Playlist">
                        <ListMusic size={18} />
                    </button>
                {/if}
                {#if kind === 'video'}
                    <button class="icon" onclick={toggleFullscreen} title="Fullscreen (f)">
                        {#if fullscreen}<Minimize size={18} />{:else}<Maximize size={18} />{/if}
                    </button>
                {/if}
            </div>
        </div>

        {#if showList && tracks.length > 1}
            <aside class="playlist">
                <h2>In this folder</h2>
                <ul>
                    {#each tracks as track (track.path)}
                        <li>
                            <button
                                class="track"
                                class:current={sameFile(track.path, path)}
                                onclick={() => go(track.path)}
                            >
                                {track.name}
                            </button>
                        </li>
                    {/each}
                </ul>
            </aside>
        {/if}
    {/if}
</div>

<style>
    .player {
        min-height: 100vh;
        display: flex;
        background: var(--color-bg, #0c0c0e);
        color: var(--color-text, #e5e5e5);
    }
    .empty,
    .unsupported {
        margin: auto;
        padding: 32px;
        text-align: center;
        color: var(--color-text-secondary, #a3a3a3);
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 10px;
    }
    .stage {
        position: relative;
        flex: 1;
        display: flex;
        flex-direction: column;
        min-width: 0;
        background: #000;
    }
    .stage.fullscreen {
        background: #000;
    }
    video {
        flex: 1;
        width: 100%;
        min-height: 0;
        object-fit: contain;
        background: #000;
        cursor: pointer;
    }
    .player.audio .stage {
        background: var(--color-panel, #151517);
    }
    .audio-face {
        flex: 1;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 14px;
        color: var(--color-text-secondary, #a3a3a3);
    }
    .audio-name {
        max-width: 80%;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: var(--color-text, #e5e5e5);
        font-weight: 500;
    }
    .error-overlay {
        position: absolute;
        inset: 0;
        display: flex;
        align-items: center;
        justify-content: center;
        background: rgba(0, 0, 0, 0.75);
        color: #fff;
        text-align: center;
        padding: 24px;
    }
    .controls {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 10px 14px;
        background: var(--color-panel, #151517);
        border-top: 1px solid var(--color-border, #222226);
    }
    .icon {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 32px;
        height: 32px;
        border-radius: 6px;
        border: none;
        background: transparent;
        color: var(--color-text, #e5e5e5);
        cursor: pointer;
    }
    .icon:hover:not(:disabled) {
        background: var(--color-panel-3, #262629);
    }
    .icon:disabled {
        opacity: 0.35;
        cursor: default;
    }
    .icon.primary {
        background: var(--color-accent, #b5352c);
        color: var(--color-accent-contrast, #fff);
    }
    .icon.primary:hover {
        background: var(--color-accent-hover, #cf4b41);
    }
    .time {
        font-variant-numeric: tabular-nums;
        font-size: 12px;
        color: var(--color-text-secondary, #a3a3a3);
        min-width: 42px;
        text-align: center;
    }
    .scrub {
        flex: 1;
        min-width: 60px;
        accent-color: var(--color-accent, #b5352c);
    }
    .volume {
        width: 80px;
        accent-color: var(--color-accent, #b5352c);
    }
    .playlist {
        width: 260px;
        flex-shrink: 0;
        border-left: 1px solid var(--color-border, #222226);
        background: var(--color-panel, #151517);
        overflow-y: auto;
        padding: 12px;
        box-sizing: border-box;
    }
    .playlist h2 {
        font-size: 12px;
        text-transform: uppercase;
        letter-spacing: 0.04em;
        color: var(--color-muted, #737373);
        margin: 0 0 8px;
    }
    .playlist ul {
        list-style: none;
        margin: 0;
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: 2px;
    }
    .track {
        width: 100%;
        text-align: left;
        background: transparent;
        border: none;
        border-radius: 6px;
        padding: 7px 8px;
        font-size: 13px;
        color: var(--color-text-secondary, #a3a3a3);
        cursor: pointer;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .track:hover {
        background: var(--color-panel-3, #262629);
    }
    .track.current {
        color: var(--color-accent, #b5352c);
        background: var(--color-accent-soft, rgba(181, 53, 44, 0.12));
    }
</style>
