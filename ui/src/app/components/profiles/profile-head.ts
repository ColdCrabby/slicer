import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  input,
  output,
  signal,
} from '@angular/core';
import { Icon, IconButton, TooltipDirective } from '@coldcrabby/ui';
import type { ProfileSource } from '../../models/profile-source';

/** What the header says about the last edit. `null` says nothing. */
export type ProfileSaveState = 'saving' | 'saved' | 'error' | null;

/** How long an armed destructive button waits before disarming itself. */
const ARM_MS = 4000;

/**
 * The top of a profile editor: the profile's name — editable where it stands —
 * what it is in one line, where it came from, and the handful of actions that
 * act on the whole profile.
 *
 * One component for the printer, filament and print-profile editors, which each
 * used to carry their own copy of this block and had drifted in the details.
 * Presentational: the page owns the store and passes values in.
 *
 * **It exists to make one thing obvious: a built-in is editable.** The old
 * header opened with a grey "Built-in" badge and a notice whose only action was
 * "Duplicate to customise", which read as "this one is locked, make a copy" —
 * when in fact every field below it could be changed in place and was saved as
 * you went. Now the name is itself an input, a built-in says in so many words
 * that edits save and apply, and the safety net is offered instead of the copy:
 * *Restore defaults* puts the shipped values back, so editing one costs nothing.
 *
 * Both destructive actions confirm inline, the design language's pattern for a
 * routine one: the first press arms the button, the second acts, and it disarms
 * on blur or after a few seconds.
 */
@Component({
  selector: 'nexus-profile-head',
  standalone: true,
  imports: [Icon, IconButton, TooltipDirective],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './profile-head.html',
  styleUrl: './profile-head.scss',
})
export class ProfileHead {
  /** Identifies the profile, so a switch to another one disarms everything. */
  readonly profileId = input.required<string>();
  readonly name = input.required<string>();
  /** Singular noun for copy: "printer", "filament", "profile". */
  readonly noun = input.required<string>();
  readonly source = input<ProfileSource | undefined>(undefined);
  /** The profile's defining facts in one line — bed and nozzle, material and heat. */
  readonly summary = input('');
  readonly isDefault = input(false);
  readonly canRestore = input(false);
  readonly saveState = input<ProfileSaveState>(null);

  readonly rename = output<string>();
  readonly makeDefault = output<void>();
  readonly duplicate = output<void>();
  readonly remove = output<void>();
  readonly restore = output<void>();

  protected readonly isBuiltin = computed(() => this.source() === 'builtin');

  protected readonly deleteArmed = signal(false);
  protected readonly restoreArmed = signal(false);
  private armTimer: ReturnType<typeof setTimeout> | null = null;

  constructor() {
    // Armed for one profile is not armed for the next: selecting another one
    // must never leave a single press away from deleting it.
    effect(() => {
      this.profileId();
      this.disarm();
    });
  }

  protected commitName(event: Event): void {
    const input = event.target as HTMLInputElement;
    const name = input.value.trim();
    if (name && name !== this.name()) {
      this.rename.emit(name);
    } else {
      // An emptied or unchanged field goes back to the name it has.
      input.value = this.name();
    }
  }

  protected finishName(event: Event): void {
    (event.target as HTMLInputElement).blur();
  }

  protected cancelName(event: Event): void {
    const input = event.target as HTMLInputElement;
    input.value = this.name();
    input.blur();
  }

  protected pressDelete(): void {
    if (this.deleteArmed()) {
      this.disarm();
      this.remove.emit();
      return;
    }
    this.arm(this.deleteArmed);
  }

  protected pressRestore(): void {
    if (this.restoreArmed()) {
      this.disarm();
      this.restore.emit();
      return;
    }
    this.arm(this.restoreArmed);
  }

  protected disarm(): void {
    if (this.armTimer !== null) {
      clearTimeout(this.armTimer);
      this.armTimer = null;
    }
    this.deleteArmed.set(false);
    this.restoreArmed.set(false);
  }

  private arm(which: typeof this.deleteArmed): void {
    this.disarm();
    which.set(true);
    this.armTimer = setTimeout(() => this.disarm(), ARM_MS);
  }
}
