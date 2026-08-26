import {
  afterNextRender,
  DestroyRef,
  Directive,
  ElementRef,
  inject,
  Renderer2,
} from '@angular/core';

// Extra px of leeway added per child to account for padding, margin, and gap
// without touching the DOM's computed styles on every resize.
const LEEWAY_PER_CHILD_PX = 16;

@Directive({
  selector: '[stackWhenCramped]',
  standalone: true,
})
export class StackWhenCramped {
  private readonly el = inject(ElementRef<HTMLElement>);
  private readonly renderer = inject(Renderer2);
  private readonly destroyRef = inject(DestroyRef);
  private observer: ResizeObserver | null = null;
  private estimatedWidth = 0;

  constructor() {
    afterNextRender(() => {
      this.estimatedWidth = this.estimateNeededWidth();
      if (this.estimatedWidth === 0) {
        return;
      }

      const el = this.el.nativeElement;
      this.observer = new ResizeObserver(() => this.updateLayout());
      this.observer.observe(el);
      this.updateLayout();
    });

    this.destroyRef.onDestroy(() => {
      this.observer?.disconnect();
    });
  }

  // Estimate the natural inline width the element needs using character counts.
  // A 1ch probe gives us the advance width for the element's font without
  // triggering layout on flex children. Each child gets a flat leeway bonus
  // to cover its padding, margin, and any gap contributed by the parent.
  private estimateNeededWidth(): number {
    const el: HTMLElement = this.el.nativeElement;
    const children = Array.from(el.children) as HTMLElement[];
    if (children.length === 0) {
      return 0;
    }

    const probe = document.createElement('span');
    probe.style.cssText =
      'position:absolute;visibility:hidden;width:1ch;display:inline-block;pointer-events:none';
    el.appendChild(probe);
    const chWidth = probe.offsetWidth || 8;
    el.removeChild(probe);

    return children.reduce((sum, child) => {
      const chars = (child.textContent ?? '').trim().length;
      return sum + chars * chWidth + LEEWAY_PER_CHILD_PX;
    }, 0);
  }

  // Compare estimated needed width against the element's own current width.
  private updateLayout(): void {
    const el: HTMLElement = this.el.nativeElement;
    const available = el.offsetWidth;

    const shouldStack = this.estimatedWidth > available;
    const isStacked = el.classList.contains('stacked');

    if (shouldStack && !isStacked) {
      this.renderer.addClass(el, 'stacked');
    } else if (!shouldStack && isStacked) {
      this.renderer.removeClass(el, 'stacked');
    }
  }
}
