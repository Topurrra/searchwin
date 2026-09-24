/*
  CamScanner store — offline document scanner state.

  The app wraps every tool in a {#key} block (CategoryWorkspace +page),
  so leaving + returning unmounts the component and wipes component-local
  $state. This module store keeps the user-produced state alive across
  navigation: the imported source image, its detected/edited corners, the
  detection flag, the enhance mode, the flattened-preview path, the OCR
  text, and a busy flag.

  Mirrors the lift-state-into-stores pattern of stores/imageStudio.ts and
  stores/ocrTool.ts. Only the ephemeral drag (which handle is being
  dragged) stays component-local.

  Corners are ALWAYS stored in FULL-RESOLUTION source-image pixel coords,
  ordered top-left, top-right, bottom-right, bottom-left — the same shape
  camscan_detect_corners returns and camscan_warp consumes. The component
  converts to/from display coords via getBoundingClientRect scaling (the
  ScreenshotRedact mouseToImage/regionStyle approach).
*/
import { writable } from 'svelte/store';

/** A corner in full-resolution source-image pixel coordinates. */
export interface CamPoint {
    x: number;
    y: number;
}

/** Backend shape from camscan_detect_corners (serde camelCase). */
export interface CamDetect {
    corners: CamPoint[];
    detected: boolean;
    width: number;
    height: number;
}
export type CamScannerPhase = 'detecting' | 'flattening' | 'exporting' | 'ocr' | null;

/** Enhance modes accepted by camscan_warp. */
export type EnhanceMode = 'color' | 'grayscale' | 'bw' | 'magic';

/** Absolute path of the imported source image, or null when none. */
export const camSourcePath = writable<string | null>(null);
/** Source image dimensions (px) as reported by detection. */
export const camSourceWidth = writable(0);
export const camSourceHeight = writable(0);
/** The 4 corners in source-image px (TL, TR, BR, BL). */
export const camCorners = writable<CamPoint[]>([]);
/** True when the backend found a confident document quad; false = bounds. */
export const camDetected = writable(false);
/** Chosen enhancement for the flatten/warp. */
export const camEnhance = writable<EnhanceMode>('magic');
/** Absolute path of the last flattened preview (temp PNG), or null. */
export const camFlattenedPath = writable<string | null>(null);
/** Recognized text from the optional OCR step. */
export const camOcrText = writable('');
/** True while any backend op (detect / warp / export) is running. */
export const camBusy = writable(false);
export const camPhase = writable<CamScannerPhase>(null);
export const camError = writable<string | null>(null);
export const camOcrOperationId = writable<string | null>(null);
export const camOcrCancelRequested = writable(false);

/** Reset everything to the no-image state. */
export function clearCamScanner() {
    camSourcePath.set(null);
    camSourceWidth.set(0);
    camSourceHeight.set(0);
    camCorners.set([]);
    camDetected.set(false);
    camFlattenedPath.set(null);
    camOcrText.set('');
    camBusy.set(false);
    camPhase.set(null);
    camError.set(null);
    camOcrOperationId.set(null);
    camOcrCancelRequested.set(false);
}

/** Image-bounds quad inset ~2% — the pure-manual / reset fallback,
 *  matching the backend's no-quad return. */
export function boundsCorners(width: number, height: number): CamPoint[] {
    const ix = width * 0.02;
    const iy = height * 0.02;
    return [
        { x: ix, y: iy },
        { x: width - ix, y: iy },
        { x: width - ix, y: height - iy },
        { x: ix, y: height - iy },
    ];
}
