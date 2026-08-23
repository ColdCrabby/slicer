import {
  ApplicationRef,
  DestroyRef,
  EnvironmentInjector,
  Injectable,
  createComponent,
  inject,
} from '@angular/core';
import type { ComponentRef, EmbeddedViewRef, TemplateRef, Type } from '@angular/core';
import { applyFloating } from './floating-core';
import type { FloatingOptions, FloatingReference } from './floating-core';

export interface FloatingConfig {
  /** Element or virtual element the panel is anchored to. */
  reference: FloatingReference;
  /** Positioning behaviour (placement, offset, flip/shift/size, …). */
  options?: FloatingOptions;
  /** Render a positioned arrow pointing at the reference. */
  arrow?: boolean;
  /** Extra class(es) applied to the floating host element. */
  panelClass?: string | readonly string[];
  /** Allow pointer interaction with the panel (menus/dropdowns). Tooltips leave this off. */
  interactive?: boolean;
  /** Element(s) whose clicks should not count as "outside" (e.g. the trigger). */
  originElement?: HTMLElement;
  /** Invoked on a pointerdown outside the panel and origin. */
  onOutsidePointer?: () => void;
  /** Invoked when Escape is pressed while the panel is open. */
  onEscape?: () => void;
}

/** Handle to an open floating panel. */
export class FloatingRef {
  private disposed = false;

  constructor(
    /** The positioned host element (holds the rendered content + arrow). */
    readonly hostElement: HTMLElement,
    private readonly arrowElement: HTMLElement | null,
    private options: FloatingOptions,
    private readonly onDispose: () => void,
    private stopPositioning: () => void,
    private reference: FloatingReference,
  ) {}

  /** Re-anchor to a new reference (e.g. a moved virtual element) and reposition. */
  setReference(reference: FloatingReference): void {
    if (this.disposed) {
      return;
    }
    this.reference = reference;
    this.stopPositioning();
    this.stopPositioning = applyFloating(this.reference, this.hostElement, {
      ...this.options,
      arrowEl: this.arrowElement,
    });
  }

  /** Recompute position immediately with (optionally) new options. */
  update(options?: Partial<FloatingOptions>): void {
    if (this.disposed) {
      return;
    }
    if (options) {
      this.options = { ...this.options, ...options };
    }
    this.stopPositioning();
    this.stopPositioning = applyFloating(this.reference, this.hostElement, {
      ...this.options,
      arrowEl: this.arrowElement,
    });
  }

  close(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.stopPositioning();
    this.onDispose();
  }
}

/** Component-flavoured handle exposing the created instance for input binding. */
export class FloatingComponentRef<C> extends FloatingRef {
  constructor(
    readonly componentRef: ComponentRef<C>,
    host: HTMLElement,
    arrowElement: HTMLElement | null,
    options: FloatingOptions,
    onDispose: () => void,
    stopPositioning: () => void,
    reference: FloatingReference,
  ) {
    super(host, arrowElement, options, onDispose, stopPositioning, reference);
  }

  get instance(): C {
    return this.componentRef.instance;
  }

  setInput(name: string, value: unknown): void {
    this.componentRef.setInput(name, value);
    this.componentRef.changeDetectorRef.detectChanges();
  }
}

/**
 * Single source of truth for floating UI (tooltips, dropdowns, popovers).
 *
 * Renders content into a body-level container that escapes every `overflow`
 * and stacking-context trap, then positions it with Floating UI so it flips,
 * shifts, and fits to stay on-screen. All app popovers funnel through here so
 * their calculation and dismissal behave identically.
 */
@Injectable({ providedIn: 'root' })
export class FloatingService {
  private readonly appRef = inject(ApplicationRef);
  private readonly envInjector = inject(EnvironmentInjector);
  private container: HTMLElement | null = null;

  constructor() {
    inject(DestroyRef).onDestroy(() => {
      this.container?.remove();
      this.container = null;
    });
  }

  /** Attach a component and position it. */
  openComponent<C>(component: Type<C>, config: FloatingConfig): FloatingComponentRef<C> {
    const { host, arrowEl } = this.createHost(config);

    const componentRef = createComponent(component, { environmentInjector: this.envInjector });
    this.appRef.attachView(componentRef.hostView);
    host.appendChild(componentRef.location.nativeElement as HTMLElement);
    if (arrowEl) {
      host.appendChild(arrowEl);
    }

    const options = this.resolveOptions(config, arrowEl);
    const stop = applyFloating(config.reference, host, options);
    const teardown = this.registerDismissal(host, config);

    return new FloatingComponentRef<C>(
      componentRef,
      host,
      arrowEl,
      config.options ?? {},
      () => {
        teardown();
        this.appRef.detachView(componentRef.hostView);
        componentRef.destroy();
        host.remove();
      },
      stop,
      config.reference,
    );
  }

  /** Attach an embedded template and position it. */
  openTemplate<C>(template: TemplateRef<C>, context: C, config: FloatingConfig): FloatingRef {
    const { host, arrowEl } = this.createHost(config);

    const viewRef = template.createEmbeddedView(context) as EmbeddedViewRef<C>;
    this.appRef.attachView(viewRef);
    for (const node of viewRef.rootNodes) {
      host.appendChild(node as Node);
    }
    if (arrowEl) {
      host.appendChild(arrowEl);
    }

    const options = this.resolveOptions(config, arrowEl);
    const stop = applyFloating(config.reference, host, options);
    const teardown = this.registerDismissal(host, config);

    return new FloatingRef(
      host,
      arrowEl,
      config.options ?? {},
      () => {
        teardown();
        this.appRef.detachView(viewRef);
        viewRef.destroy();
        host.remove();
      },
      stop,
      config.reference,
    );
  }

  private createHost(config: FloatingConfig): { host: HTMLElement; arrowEl: HTMLElement | null } {
    const container = this.ensureContainer();
    const host = document.createElement('div');
    host.className = 'nexus-floating';
    if (config.interactive) {
      host.style.pointerEvents = 'auto';
    }
    if (config.panelClass) {
      const classes = Array.isArray(config.panelClass) ? config.panelClass : [config.panelClass];
      host.classList.add(...classes);
    }

    let arrowEl: HTMLElement | null = null;
    if (config.arrow) {
      arrowEl = document.createElement('div');
      arrowEl.className = 'nexus-floating-arrow';
    }

    container.appendChild(host);
    return { host, arrowEl };
  }

  private resolveOptions(config: FloatingConfig, arrowEl: HTMLElement | null): FloatingOptions {
    return { ...config.options, arrowEl };
  }

  private registerDismissal(host: HTMLElement, config: FloatingConfig): () => void {
    const cleanups: Array<() => void> = [];

    if (config.onOutsidePointer) {
      const onPointer = (event: PointerEvent): void => {
        const target = event.target as Node;
        if (host.contains(target) || config.originElement?.contains(target)) {
          return;
        }
        config.onOutsidePointer!();
      };
      document.addEventListener('pointerdown', onPointer, true);
      cleanups.push(() => document.removeEventListener('pointerdown', onPointer, true));
    }

    if (config.onEscape) {
      const onKey = (event: KeyboardEvent): void => {
        if (event.key === 'Escape') {
          config.onEscape!();
        }
      };
      document.addEventListener('keydown', onKey, true);
      cleanups.push(() => document.removeEventListener('keydown', onKey, true));
    }

    return () => {
      for (const c of cleanups) {
        c();
      }
    };
  }

  private ensureContainer(): HTMLElement {
    if (this.container?.isConnected) {
      return this.container;
    }
    const container = document.createElement('div');
    container.className = 'nexus-floating-container';
    document.body.appendChild(container);
    this.container = container;
    return container;
  }
}
