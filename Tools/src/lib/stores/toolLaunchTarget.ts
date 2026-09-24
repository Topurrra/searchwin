import { writable, type Unsubscriber } from 'svelte/store';

export type ToolLaunchTarget = {
    toolId: string;
    targetFile: string;
};

export const toolLaunchTarget = writable<ToolLaunchTarget | null>(null);

export function stageToolLaunchTarget(target: ToolLaunchTarget) {
    const targetFile = target.targetFile.trim();
    if (!targetFile) return;
    toolLaunchTarget.set({ ...target, targetFile });
}

export function subscribeToToolLaunchTarget(
    toolId: string,
    receive: (target: ToolLaunchTarget) => void,
): Unsubscriber {
    return toolLaunchTarget.subscribe((target) => {
        if (!target || target.toolId !== toolId) return;
        toolLaunchTarget.set(null);
        receive(target);
    });
}
