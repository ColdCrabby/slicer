/**
 * Dispose all GPU-backed resources (geometry + materials) reachable from
 * `obj` by traversing its scene-graph subtree. Safe to call on any
 * Three.js `Object3D` or on plain objects (no-op if the expected methods
 * aren't present).
 */
export function disposeObject(obj: unknown): void {
  const node = obj as {
    traverse?: (cb: (child: unknown) => void) => void;
    geometry?: { dispose?: () => void };
    material?: { dispose?: () => void } | { dispose?: () => void }[];
  };
  if (typeof node.traverse === 'function') {
    node.traverse((child) => {
      const c = child as {
        geometry?: { dispose?: () => void };
        material?: { dispose?: () => void } | { dispose?: () => void }[];
      };
      c.geometry?.dispose?.();
      const mat = c.material;
      if (Array.isArray(mat)) {
        for (const m of mat) {
          m.dispose?.();
        }
      } else {
        mat?.dispose?.();
      }
    });
  }
}
