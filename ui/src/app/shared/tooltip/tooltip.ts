import { ChangeDetectionStrategy, Component, ViewEncapsulation, input } from '@angular/core';
import { MarkdownComponent } from 'ngx-markdown';

@Component({
  selector: 'nexus-tooltip',
  templateUrl: './tooltip.component.html',
  styleUrl: './tooltip.component.css',
  changeDetection: ChangeDetectionStrategy.OnPush,
  encapsulation: ViewEncapsulation.None,
  imports: [MarkdownComponent],
})
export class Tooltip {
  readonly text = input.required<string>();
  readonly mode = input<'inline' | 'block'>('inline');
  readonly shortcut = input<string | undefined>(undefined);
  readonly persistent = input<boolean>(false);
}
