import { Component, computed, inject } from '@angular/core';
import { Slicer } from '../../services/slicer';
import { Badge, BadgeVariant } from '@coldcrabby/ui';

interface StatusConfig {
  label: string;
  variant: BadgeVariant;
  pulse: boolean;
}

const STATUS_CONFIG: Record<string, StatusConfig> = {
  connecting: { label: 'Connecting…', variant: 'warning', pulse: true },
  connected: { label: 'Connected', variant: 'success', pulse: false },
  disconnected: { label: 'Disconnected', variant: 'default', pulse: false },
  failed: { label: '', variant: 'danger', pulse: false },
};

@Component({
  selector: 'nexus-connection-state',
  standalone: true,
  imports: [Badge],
  templateUrl: './connection-state.html',
  styleUrl: './connection-state.scss',
  host: {
    '[hidden]': '!slicer.shouldShowConnectionStatus()',
  },
})
export class ConnectionState {
  readonly slicer = inject(Slicer);

  readonly config = computed<StatusConfig>(
    () => STATUS_CONFIG[this.slicer.connectionStatus()] ?? STATUS_CONFIG['disconnected'],
  );
}
