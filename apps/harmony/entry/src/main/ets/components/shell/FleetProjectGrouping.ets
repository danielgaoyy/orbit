import {
  FleetSidebarDto,
  SidebarConversationDto,
  SidebarProjectGroupDto
} from '../../model/OrbitTypes';

export const MAX_VISIBLE_THREADS_PER_PROJECT = 5;

export function filterSidebarConversationsForHost(
  items: SidebarConversationDto[] | undefined,
  hostId: string
): SidebarConversationDto[] {
  if (items === undefined || items.length === 0) {
    return [];
  }
  if (hostId.length === 0) {
    return items;
  }
  const out: SidebarConversationDto[] = [];
  for (let i = 0; i < items.length; i++) {
    if (items[i].host_id === hostId) {
      out.push(items[i]);
    }
  }
  return out;
}

export function filterSidebarProjectsForHost(
  sidebar: FleetSidebarDto | null,
  hostId: string
): SidebarProjectGroupDto[] {
  if (sidebar === null || sidebar.projects.length === 0) {
    return [];
  }
  if (hostId.length === 0) {
    return sidebar.projects;
  }
  const out: SidebarProjectGroupDto[] = [];
  for (let i = 0; i < sidebar.projects.length; i++) {
    if (sidebar.projects[i].host_id === hostId) {
      out.push(sidebar.projects[i]);
    }
  }
  return out;
}
