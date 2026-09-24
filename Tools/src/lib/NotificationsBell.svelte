<script lang="ts">
    import { notifications, unreadCount, markRead, markAllRead, clearNotifications } from '$lib/stores/notifications';
    import { Bell, CheckCircle2, AlertTriangle, XCircle, Info } from '@lucide/svelte';
    import { _ } from 'svelte-i18n';

    let open = $state(false);

    function fmtTime(ts: number): string {
        const diff = Date.now() - ts;
        if (diff < 60_000) return $_('nav.timeJustNow');
        if (diff < 3600_000) return $_('nav.timeMinutesAgo', { values: { count: Math.floor(diff / 60_000) } });
        if (diff < 86400_000) return $_('nav.timeHoursAgo', { values: { count: Math.floor(diff / 3600_000) } });
        return new Date(ts).toLocaleString();
    }

    function toggle() {
        open = !open;
        if (open) markAllRead();
    }

    function close() {
        open = false;
    }
</script>

<svelte:window onclick={(e) => {
  const target = e.target as HTMLElement;
  if (open && !target.closest('[data-notifications]')) close();
}} />

<div class="relative" data-notifications>
    <button
            onclick={toggle}
            class="relative p-2 rounded hover:bg-panel-2 transition-colors"
            title={$_('nav.notifications')}
    >
        <Bell class="w-4 h-4 text-text-secondary" />
        {#if $unreadCount > 0}
      <span class="absolute -top-0.5 -right-0.5 min-w-[16px] h-[16px] px-1 rounded-full bg-accent text-accent-contrast text-[10px] font-bold flex items-center justify-center">
        {$unreadCount > 9 ? '9+' : $unreadCount}
      </span>
        {/if}
    </button>

    {#if open}
        <div class="fade-in fixed right-3 top-14 w-[min(calc(100vw-1.5rem),20rem)] max-h-[min(480px,calc(100dvh-4rem))] bg-panel border border-border rounded-2xl shadow-xl overflow-hidden flex flex-col z-[80]">
            <div class="px-4 py-3 border-b border-border flex items-center justify-between">
                <span class="text-sm font-semibold">{$_('nav.notifications')}</span>
                {#if $notifications.length > 0}
                    <button onclick={clearNotifications} class="text-xs text-muted hover:text-text">{$_('nav.clearAll')}</button>
                {/if}
            </div>

            <div class="flex-1 overflow-y-auto">
                {#if $notifications.length === 0}
                    <div class="p-8 text-center text-muted text-sm">
                        <Bell class="w-8 h-8 mx-auto mb-2 opacity-50" />
                        <div>{$_('nav.noNotifications')}</div>
                    </div>
                {:else}
                    {#each $notifications as n (n.id)}
                        <button
                                onclick={() => markRead(n.id)}
                                class="w-full text-left px-4 py-3 border-b border-border hover:bg-panel-2 transition-colors flex items-start gap-3"
                        >
                            <div class="shrink-0 mt-0.5">
                                {#if n.level === 'success'}
                                    <CheckCircle2 class="w-4 h-4 text-success" />
                                {:else if n.level === 'error'}
                                    <XCircle class="w-4 h-4 text-error" />
                                {:else if n.level === 'warning'}
                                    <AlertTriangle class="w-4 h-4 text-warning" />
                                {:else}
                                    <Info class="w-4 h-4 text-muted" />
                                {/if}
                            </div>
                            <div class="flex-1 min-w-0">
                                <div class="text-sm font-medium text-text truncate">{n.title}</div>
                                {#if n.message}
                                    <div class="text-xs text-text-secondary mt-0.5 line-clamp-2">{n.message}</div>
                                {/if}
                                <div class="text-[10px] text-muted mt-1">{fmtTime(n.timestamp)}</div>
                            </div>
                        </button>
                    {/each}
                {/if}
            </div>
        </div>
    {/if}
</div>
