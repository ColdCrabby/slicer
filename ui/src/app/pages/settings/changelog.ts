import { ChangeDetectionStrategy, Component, inject, OnInit } from '@angular/core';
import { ChangelogList } from '../../components/changelog/changelog-list';
import { AppVersion } from '../../services/app-version';
import { SectionHeader } from '@coldcrabby/ui';

/**
 * The complete release history. Mounts the same {@link ChangelogList} the
 * post-upgrade dialog uses, so both surfaces stay identical — this page is also
 * where native shells (whose dialogs are drawn by the OS and cannot hold this
 * much content) send the user after an update.
 */
@Component({
  selector: 'nexus-settings-changelog',
  imports: [ChangelogList, SectionHeader],
  templateUrl: './changelog.html',
  styleUrl: './changelog.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ChangelogSettings implements OnInit {
  private readonly appVersion = inject(AppVersion);

  protected readonly entries = this.appVersion.changelog;
  protected readonly currentVersion = this.appVersion.currentChangelogVersion;

  ngOnInit(): void {
    void this.appVersion.loadInfo();
    void this.appVersion.loadChangelog();
  }
}
