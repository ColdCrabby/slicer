import { ChangeDetectionStrategy, Component, input } from '@angular/core';

@Component({
  selector: 'nexus-logo',
  templateUrl: './logo.html',
  styleUrl: './logo.css',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class Logo {
  readonly hideProductName = input<boolean>(false);
}
