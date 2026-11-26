import { basenameOfPath } from '../../model/OrbitTypes';

export interface TopBarDisplayInput {
  readonly hasPairedHosts: boolean;
  readonly isShowingPaSetup: boolean;
  readonly isPersonalAgentSession: boolean;
  readonly isDraft: boolean;
  readonly currentAgentTitle: string;
  readonly activeCwd: string;
  readonly hostName: string;
  readonly hostCount: number;
  readonly isConnectionHealthy: boolean;
  readonly connectionState: string;
  readonly viaSshTunnel: boolean;
  readonly gitBranch: string;
}

export interface TopBarDisplayModel {
  readonly titleKind: 'orbit' | 'pa' | 'new_conv' | 'text';
  readonly titleText: string;
  readonly subtitleKind:
    | 'no_hosts'
    | 'hosts_count'
    | 'select_project'
    | 'new_conv'
    | 'host_select_project'
    | 'conn_target'
    | 'text';
  readonly subtitleText: string;
  readonly subtitleHostName: string;
  readonly subtitleTarget: string;
  readonly isCwdUnset: boolean;
}

export function isWorkspaceCwdUnset(activeCwd: string): boolean {
  const clean = activeCwd.trim();
  return clean.length === 0 || clean === '~';
}

export function resolveConnectionStateResource(state: string, viaSsh: boolean): Resource {
  if (state === 'online_direct') {
    return viaSsh ? $r('app.string.conn_ssh') : $r('app.string.conn_direct');
  }
  if (state === 'online_relay') {
    return $r('app.string.conn_relay');
  }
  if (state === 'connecting_direct' || state === 'connecting_relay') {
    return $r('app.string.conn_connecting');
  }
  return $r('app.string.conn_offline');
}

/**
 * Pure top-bar title & subtitle resolver guaranteeing:
 * - Never emits `'~'` anywhere in title or subtitle.
 * - Draft session with empty/`~` cwd: line 1 = host name, line 2 = `$r('app.string.sidebar_selectProject')`.
 * - Titled regular session or Personal Agent with empty/`~` cwd: preserves session/PA title on line 1
 *   AND keeps host name visible alongside `$r('app.string.sidebar_selectProject')` on line 2.
 */
export function resolveTopBarDisplayModel(input: TopBarDisplayInput): TopBarDisplayModel {
  const hostName = input.hostName.trim();
  const cwdUnset = isWorkspaceCwdUnset(input.activeCwd);

  if (!input.hasPairedHosts) {
    return {
      titleKind: 'orbit',
      titleText: 'Orbit',
      subtitleKind: 'no_hosts',
      subtitleText: '',
      subtitleHostName: '',
      subtitleTarget: '',
      isCwdUnset: cwdUnset
    };
  }

  if (input.isShowingPaSetup) {
    return {
      titleKind: 'pa',
      titleText: '',
      subtitleKind: 'hosts_count',
      subtitleText: '',
      subtitleHostName: hostName,
      subtitleTarget: '',
      isCwdUnset: cwdUnset
    };
  }

  if (input.isDraft) {
    const repo = cwdUnset ? hostName : basenameOfPath(input.activeCwd);
    return {
      titleKind: repo.length > 0 ? 'text' : 'new_conv',
      titleText: repo,
      subtitleKind: cwdUnset ? 'select_project' : 'new_conv',
      subtitleText: '',
      subtitleHostName: hostName,
      subtitleTarget: repo,
      isCwdUnset: cwdUnset
    };
  }

  // Active session (Personal Agent or regular conversation)
  let titleKind: 'orbit' | 'pa' | 'new_conv' | 'text' = 'new_conv';
  let titleText = '';
  const cleanTitle = input.currentAgentTitle.trim();
  if (input.isPersonalAgentSession) {
    titleKind = 'pa';
  } else if (cleanTitle.length > 0) {
    titleKind = 'text';
    titleText = cleanTitle;
  } else if (cwdUnset && hostName.length > 0) {
    titleKind = 'text';
    titleText = hostName;
  }

  if (cwdUnset) {
    // Keep host name visible in subtitle whenever line 1 is NOT already showing the host name.
    const titleShowsHost = titleKind === 'text' && titleText === hostName && hostName.length > 0;
    if (!titleShowsHost && hostName.length > 0) {
      return {
        titleKind: titleKind,
        titleText: titleText,
        subtitleKind: 'host_select_project',
        subtitleText: '',
        subtitleHostName: hostName,
        subtitleTarget: '',
        isCwdUnset: true
      };
    }
    return {
      titleKind: titleKind,
      titleText: titleText,
      subtitleKind: 'select_project',
      subtitleText: '',
      subtitleHostName: hostName,
      subtitleTarget: '',
      isCwdUnset: true
    };
  }

  const repo = basenameOfPath(input.activeCwd);
  const isMultiHost = input.hostCount > 1;
  if (!input.isConnectionHealthy) {
    const target = isMultiHost && hostName.length > 0 ? hostName : repo;
    return {
      titleKind: titleKind,
      titleText: titleText,
      subtitleKind: 'conn_target',
      subtitleText: '',
      subtitleHostName: hostName,
      subtitleTarget: target,
      isCwdUnset: false
    };
  }
  if (isMultiHost && hostName.length > 0) {
    return {
      titleKind: titleKind,
      titleText: titleText,
      subtitleKind: 'text',
      subtitleText: `${hostName} / ${repo}`,
      subtitleHostName: hostName,
      subtitleTarget: repo,
      isCwdUnset: false
    };
  }
  const branch = input.gitBranch.trim();
  return {
    titleKind: titleKind,
    titleText: titleText,
    subtitleKind: 'text',
    subtitleText: branch.length > 0 ? `${repo} · ${branch}` : repo,
    subtitleHostName: hostName,
    subtitleTarget: repo,
    isCwdUnset: false
  };
}
