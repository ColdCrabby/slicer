import { Directive, ElementRef, HostListener } from '@angular/core';
import { isTauriHost } from '../runtime/domain/runtime-mode.util';

@Directive({
  selector: 'a[href^="http://"], a[href^="https://"]',
  standalone: true,
})
export class ExternalLinkDirective {
  constructor(private el: ElementRef<HTMLAnchorElement>) {}

  @HostListener('click', ['$event']) onClick(event: MouseEvent): void {
    if (!isTauriHost()) {
      return;
    }

    event.preventDefault();
    const href = this.el.nativeElement.getAttribute('href');
    if (!href) {
      return;
    }

    import('@tauri-apps/plugin-shell').then(({ open }) => {
      open(href);
    });
  }
}
