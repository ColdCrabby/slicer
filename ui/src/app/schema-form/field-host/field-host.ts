import {
  ChangeDetectionStrategy,
  Component,
  ComponentRef,
  DestroyRef,
  computed,
  effect,
  inject,
  input,
  output,
  untracked,
  viewChild,
  ViewContainerRef,
} from '@angular/core';
import { Subscription } from 'rxjs';
import { InlineNotice } from '../../ui/inline-notice/inline-notice';
import { noticeForField } from '../field-exceptions/field-exceptions';
import { resolveWidget } from '../field-registry/field-registry';
import { FieldDef } from '../models/field-def';
import { FieldWidget } from '../widgets/base-field';

/**
 * Dynamic host that instantiates the correct widget component for a given
 * `FieldDef` at runtime using `ViewContainerRef.createComponent()`.
 *
 * This approach is required (rather than `NgComponentOutlet`) because
 * `NgComponentOutlet` does not support binding to outputs dynamically.
 *
 * Beneath the widget it renders any field-specific {@link noticeForField}
 * exception (e.g. the raft-adhesion caution) so the note sits directly against
 * the control it refers to, without the widget-choosing path knowing about it.
 */
@Component({
  selector: 'se-field-host',
  standalone: true,
  imports: [InlineNotice],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <ng-container #host />
    @if (notice(); as n) {
      <nexus-inline-notice
        class="field-notice"
        [tone]="n.tone ?? 'info'"
        [icon]="n.icon"
        [title]="n.title"
      >
        {{ n.text }}
      </nexus-inline-notice>
    }
  `,
  styles: `
    .field-notice {
      margin-top: var(--spacing-sm);
    }
  `,
})
export class FieldHost {
  readonly field = input.required<FieldDef>();
  readonly value = input<unknown>(undefined);
  readonly fieldChange = output<unknown>();

  /** Field-specific caution/hint to render under the control, if any. */
  protected readonly notice = computed(() => noticeForField(this.field(), this.value()));

  private readonly vcr = viewChild.required('host', { read: ViewContainerRef });
  private readonly destroyRef = inject(DestroyRef);

  private componentRef: ComponentRef<FieldWidget> | null = null;
  private changeSubscription: Subscription | null = null;

  constructor() {
    effect(() => {
      this.field(); // tracked — recreate widget when field changes
      untracked(() => {
        this.destroyWidget();
        this.createWidget();
      });
    });

    effect(() => {
      const value = this.value(); // tracked — update input when value changes
      untracked(() => {
        this.componentRef?.setInput('value', value);
      });
    });

    this.destroyRef.onDestroy(() => this.destroyWidget());
  }

  private createWidget(): void {
    const componentClass = resolveWidget(this.field());

    this.componentRef = this.vcr().createComponent(componentClass);
    this.componentRef.setInput('field', this.field());
    this.componentRef.setInput('value', this.value());

    this.changeSubscription = this.componentRef.instance.valueChange.subscribe((val) => {
      this.fieldChange.emit(val);
    });
  }

  private destroyWidget(): void {
    this.changeSubscription?.unsubscribe();
    this.changeSubscription = null;
    this.componentRef?.destroy();
    this.componentRef = null;
  }
}
