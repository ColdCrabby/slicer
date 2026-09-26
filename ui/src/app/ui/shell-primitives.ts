/**
 * The `@coldcrabby/ui` primitives the app shell needs before its first paint —
 * and nothing else.
 *
 * **Code that is statically reachable from `main.ts` imports from here, never
 * from `@coldcrabby/ui`.** esbuild assigns a module to a chunk by following
 * every `export … from` edge of the files that reach it, so a single eager
 * import of the package barrel pulls every primitive it re-exports into the
 * initial bundle, whether or not anything eager renders it. The colour picker,
 * select, sliders and radio groups only the settings and slice pages use were
 * riding in `main` that way.
 *
 * Lazy code keeps importing the barrel: it resolves to the same module files,
 * so nothing is duplicated. Add to this list only what the shell itself uses.
 */
export { Button } from '../../../vendor/coldcrabby-ui/src/lib/ui/button/button';
export { IconButton } from '../../../vendor/coldcrabby-ui/src/lib/ui/icon-button/icon-button';
export { Icon } from '../../../vendor/coldcrabby-ui/src/lib/shared/icon/icon';
export { Badge } from '../../../vendor/coldcrabby-ui/src/lib/shared/badge/badge';
export type { BadgeVariant } from '../../../vendor/coldcrabby-ui/src/lib/shared/badge/badge';
export { TooltipDirective } from '../../../vendor/coldcrabby-ui/src/lib/shared/tooltip/tooltip.directive';
export { UserInputModality } from '../../../vendor/coldcrabby-ui/src/lib/shared/input-modality/input-modality';
export {
  FloatingService,
  FloatingComponentRef,
  FloatingRef,
} from '../../../vendor/coldcrabby-ui/src/lib/shared/floating/floating.service';
export type { FloatingReference } from '../../../vendor/coldcrabby-ui/src/lib/shared/floating/floating-core';
