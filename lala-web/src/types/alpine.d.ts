/**
 * Minimal type declarations for Alpine.js 3.x global usage.
 *
 * Alpine components are registered as global functions that return
 * plain objects. Alpine.js calls these functions when it encounters
 * x-data="functionName()" in the DOM.
 */

/** Alpine.js $data magic property — access parent x-data scope in nested components */
interface AlpineScope {
  $data: Record<string, unknown>;
}

/** Extend Window with Alpine component functions registered at runtime */
// eslint-disable-next-line @typescript-eslint/no-empty-interface
interface Window {
  [key: string]: unknown;
}
