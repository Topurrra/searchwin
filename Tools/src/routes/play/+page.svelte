<script lang="ts">
    // Search's own media player: https://tools.search/#/play?path=<encoded>.
    // Reached from the field's Play action (Browser.Field.cs → ToolsHost) for
    // a video or audio file, never typed at a web page — files.search only
    // serves tool pages (see ToolsHost.cs), and this page only ever asks it
    // for the path the browser handed it or a sibling the engine listed.
    import { invoke, convertFileSrc } from '@tauri-apps/api/core';
    import { listen } from '@tauri-apps/api/event';
    import { untrack } from 'svelte';
    import * as positions from '$lib/player/positions';
    import { folderOf, inFolder, after } from '$lib/player/positions';
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
        Captions,
    } from '@lucide/svelte';

    // What WebView2 (Chromium) plays as it is, and what it plays once the
    // FFmpeg pack has remuxed it (the browser's Player.cs). Kept in sync by
    // hand with SearchKit.Field.FileKinds (Playable, Remuxable) — see
    // Search.Kit/Field/FieldRow.cs and its tests.
    const VIDEO_EXT = new Set(['mp4', 'm4v', 'webm', 'mov', 'ogv']);
    const AUDIO_EXT = new Set(['mp3', 'm4a', 'aac', 'wav', 'ogg', 'oga', 'opus', 'flac']);
    const REMUX_VIDEO_EXT = new Set(['mkv', 'avi', 'wmv', 'flv', 'mpg', 'mpeg', 'm2ts', 'mts', '3gp']);
    const REMUX_AUDIO_EXT = new Set(['wma', 'mka', 'ape', 'wv', 'aiff', 'aif']);

    type Kind = 'video' | 'audio' | 'unsupported';

    function extOf(path: string): string {
        const m = /\.([^.\\/]+)$/.exec(path);
        return m ? m[1].toLowerCase() : '';
    }
    function kindOf(path: string): Kind {
        const ext = extOf(path);
        if (VIDEO_EXT.has(ext) || REMUX_VIDEO_EXT.has(ext)) return 'video';
        if (AUDIO_EXT.has(ext) || REMUX_AUDIO_EXT.has(ext)) return 'audio';
        return 'unsupported';
    }
    function remuxes(path: string): boolean {
        const ext = extOf(path);
        return REMUX_VIDEO_EXT.has(ext) || REMUX_AUDIO_EXT.has(ext);
    }
    function baseName(path: string): string {
        const m = /[^\\/]+$/.exec(path);
        return m ? m[0] : path;
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
    // MARK: - what WebView2 can't play as it is: the FFmpeg pack

    // How the file is being played: `direct`ly, or made playable by the
    // browser (Player.cs) — streams copied (`remux`), or the video
    // converted too (`transcode`, when a copy didn't play). A playback
    // error moves one step along, and past `transcode` it's an error.
    type How = 'direct' | 'remux' | 'transcode';
    interface Prepared {
        media?: string;
        subtitles?: { path: string; label: string; language: string }[];
        needsPack?: boolean;
        playable?: boolean;
    }
    let how = $state<How>('direct');
    let prepared = $state<Prepared | null>(null);
    let preparing = $state(false);
    let progress = $state<number | null>(null);
    let needsPack = $state(false);
    let prepareError = $state<string | null>(null);
    let captions = $state(0);

    const src = $derived(
        !path ? '' : how === 'direct' ? convertFileSrc(path) : prepared?.media ? convertFileSrc(prepared.media) : '',
    );
    const subtitles = $derived(prepared?.subtitles ?? []);

    // `check`: a file WebView2 should play as it is — does it decode every
    // stream in it (an MP4 with AC-3 would play silent), and are there
    // subtitle files beside it? Quiet: it plays meanwhile.
    async function prepare(file: string, wanted: How | 'check') {
        const quiet = wanted === 'check';
        if (!quiet) {
            how = wanted as How;
            preparing = true;
            progress = null;
            prepared = null;
        }
        try {
            const answer = await invoke<Prepared>('host:play.prepare', { path: file, how: wanted });
            if (file !== path) return;
            if (answer?.needsPack) needsPack = true;
            else if (quiet && answer?.playable === false) void prepare(file, 'remux');
            else if (quiet) prepared = { subtitles: answer?.subtitles ?? [] };
            else prepared = answer;
        } catch (error) {
            if (file === path && !quiet) prepareError = String(error);
        } finally {
            if (file === path && !quiet) preparing = false;
        }
    }

    // A new file: straight into the player if WebView2 plays it (with any
    // subtitle files beside it), made playable first if it doesn't.
    $effect(() => {
        const file = path;
        if (!file || kindOf(file) === 'unsupported') return;
        how = 'direct';
        prepared = null;
        needsPack = false;
        prepareError = null;
        preparing = false;
        if (remuxes(file)) void prepare(file, 'remux');
        else void prepare(file, 'check');
        return () => {
            if (preparing) void invoke('host:play.cancel', { path: file }).catch(() => {});
        };
    });

    function onMediaError() {
        if (!media || !src || media.currentSrc !== src) return;
        const next: How | null = how === 'direct' ? 'remux' : how === 'remux' && kindOf(path) === 'video' ? 'transcode' : null;
        if (next) void prepare(path, next);
        else mediaError = true;
    }

    // Progress from the browser while it makes the file playable, and the
    // pack arriving while the page waits for it.
    $effect(() => {
        const stops = [
            listen<{ path: string; fraction: number | null }>('play-progress', ({ payload }) => {
                if (payload?.path?.toLowerCase() === path.replace(/\//g, '\\').toLowerCase()) progress = payload.fraction;
            }),
            listen<{ id: string; installed: string | null }>('packs-changed', ({ payload }) => {
                if (payload?.id === 'ffmpeg' && payload.installed && needsPack) {
                    needsPack = false;
                    void prepare(path, how === 'direct' ? 'remux' : how);
                }
            }),
        ];
        return () => stops.forEach((stop) => void stop.then((unlisten) => unlisten()));
    });

    function getPack() {
        void invoke('host:packs.open').catch(() => {});
    }

    // Subtitles: off, or one of the tracks — the first is on to begin with.
    function showCaptions(which: number) {
        captions = which;
        const tracks = media?.textTracks;
        if (!tracks) return;
        for (let i = 0; i < tracks.length; i++) tracks[i].mode = i === which - 1 ? 'showing' : 'disabled';
    }
    function nextCaptions() {
        showCaptions((captions + 1) % (subtitles.length + 1));
    }
    // Tracks arrive one by one (and Chromium may switch one on itself):
    // whatever's chosen is applied again as each comes and as the media loads.
    $effect(() => {
        const tracks = media?.textTracks;
        if (!tracks || subtitles.length === 0) return;
        captions = 1;
        const apply = () => showCaptions(untrack(() => captions));
        tracks.addEventListener?.('addtrack', apply);
        media?.addEventListener('loadedmetadata', apply);
        apply();
        return () => {
            tracks.removeEventListener?.('addtrack', apply);
            media?.removeEventListener('loadedmetadata', apply);
        };
    });

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
                    .map((c) => ({ name: c.name, path: inFolder(wanted, c.name), kind: kindOf(c.name) }))
                    .filter((t) => t.kind !== 'unsupported');
            })
            .catch(() => {
                if (folder === wanted) tracks = [];
            });
    });

    const index = $derived(tracks.findIndex((t) => sameFile(t.path, path)));

    // The tab's address — what bench, a bookmark, and the tab's own title
    // would see — names the file actually playing, set by hand rather than
    // through `goto`: SvelteKit's hash router only matches the route from
    // the hash, and drops a `?path=` living inside it on the way back out.
    // `replaceState`, not `pushState`: next/previous/auto-advance and a
    // playlist click change the file playing, not a place worth walking
    // back to one track at a time. The URL still changes, so the browser's
    // same-document navigation sees a new address to follow.
    function go(newPath: string) {
        history.replaceState(history.state, '', `#/play?path=${encodeURIComponent(newPath)}`);
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

    // The last 200 files played; a finished one is forgotten (see
    // $lib/player/positions).
    function loadPosition(p: string): number {
        try {
            return positions.load(localStorage, p);
        } catch {
            return 0;
        }
    }
    function savePosition(p: string, time: number) {
        try {
            positions.save(localStorage, p, time, media?.duration ?? NaN);
        } catch {
            /* storage disabled — just don't remember */
        }
    }

    // Where the position was last saved. Saving used to wait for playback to
    // move two seconds PAST it, so a seek back while paused (after a track
    // had ended and saved 0, say) was never saved — and the pause button,
    // on media already paused, fires no pause event: the file came back at
    // 0. Now any move of two seconds either way saves, and so does every
    // seek.
    let lastSaved = 0;
    // Events from the file just left (a timeupdate as the source changes)
    // are not the new file's position.
    function playingPath(): boolean {
        return !!media && !!src && media.currentSrc === src;
    }
    function remember() {
        if (!media || !playingPath()) return;
        lastSaved = media.currentTime;
        savePosition(path, media.currentTime);
    }
    function onTimeUpdate() {
        if (!media) return;
        current = media.currentTime;
        // Once every couple of seconds, not every frame.
        if (Math.abs(current - lastSaved) >= 2) remember();
        setPositionState();
    }
    function onSeeked() {
        if (media) current = media.currentTime;
        remember();
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
        // Finished: forgotten, so it starts over next time.
        lastSaved = 0;
        savePosition(path, 0);
        // On through the folder, stopping after its last track.
        const following = after(index, tracks.length);
        if (following >= 0) go(tracks[following].path);
    }
    function onPause() {
        playing = false;
        if (media && !media.ended) remember();
    }
    // Reloading or closing the tab keeps where it was, to the second.
    $effect(() => {
        const leaving = () => remember();
        window.addEventListener('pagehide', leaving);
        return () => window.removeEventListener('pagehide', leaving);
    });

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
            case 'c':
            case 'C':
                if (subtitles.length > 0) nextCaptions();
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
            <p>Search can't play “{name}”.</p>
        </div>
    {:else}
        <div class="stage" bind:this={stage} class:fullscreen>
            {#if kind === 'video'}
                <!-- Subtitles come from files.search, another origin: asked for with CORS. -->
                <!-- svelte-ignore a11y_media_has_caption -->
                <video
                    bind:this={media}
                    {src}
                    crossorigin="anonymous"
                    ontimeupdate={onTimeUpdate}
                    onloadedmetadata={onLoadedMetadata}
                    onseeked={onSeeked}
                    onended={onEnded}
                    onplay={() => (playing = true)}
                    onpause={onPause}
                    onerror={onMediaError}
                    onclick={togglePlay}
                    ondblclick={toggleFullscreen}
                >
                    {#each subtitles as sub (sub.path)}
                        <track kind="subtitles" src={convertFileSrc(sub.path)} label={sub.label} srclang={sub.language || undefined} />
                    {/each}
                </video>
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
                    onseeked={onSeeked}
                    onended={onEnded}
                    onplay={() => (playing = true)}
                    onpause={onPause}
                    onerror={onMediaError}
                ></audio>
            {/if}

            {#if needsPack}
                <div class="error-overlay">
                    <p>“{name}” plays with the FFmpeg pack, which isn't installed.</p>
                    <button class="get-pack" onclick={getPack}>Get the FFmpeg pack</button>
                </div>
            {:else if preparing}
                <div class="error-overlay" aria-live="polite">
                    <p>
                        Getting “{name}” ready to play…{#if progress != null}
                            {Math.round(progress * 100)}%{/if}
                    </p>
                </div>
            {:else if prepareError || mediaError}
                <div class="error-overlay">
                    <p>“{name}” couldn't be played{prepareError ? `: ${prepareError}` : '.'}</p>
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
                {#if subtitles.length > 0}
                    <button
                        class="icon"
                        class:on={captions > 0}
                        onclick={nextCaptions}
                        title={captions > 0 ? `Subtitles: ${subtitles[captions - 1]?.label} (c)` : 'Subtitles off (c)'}
                    >
                        <Captions size={18} />
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
        flex-direction: column;
        gap: 12px;
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
    .icon.on {
        color: var(--color-accent, #b5352c);
    }
    .get-pack {
        border: none;
        border-radius: 6px;
        padding: 7px 14px;
        cursor: pointer;
        background: var(--color-accent, #b5352c);
        color: var(--color-accent-contrast, #fff);
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
