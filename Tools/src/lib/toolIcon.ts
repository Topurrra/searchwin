// A tool's own icon in its tab: the Lucide symbol it has in the index,
// drawn in Search's muted grey, which reads on a light strip and a dark one.
import { flushSync, mount, unmount, type Component } from 'svelte';

export function wearIcon(icon: Component<any> | undefined) {
    let href = '';
    if (icon) {
        const box = document.createElement('div');
        const drawn = mount(icon, { target: box, props: { size: 32, color: '#8c8c8c', strokeWidth: 2 } });
        flushSync();
        const svg = box.querySelector('svg');
        if (svg) {
            svg.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
            href = `data:image/svg+xml,${encodeURIComponent(svg.outerHTML)}`;
        }
        unmount(drawn);
    }
    let link = document.head.querySelector<HTMLLinkElement>('link[rel="icon"]');
    if (!href) {
        link?.remove();
        return;
    }
    if (!link) {
        link = document.createElement('link');
        link.rel = 'icon';
        document.head.append(link);
    }
    link.href = href;
}
