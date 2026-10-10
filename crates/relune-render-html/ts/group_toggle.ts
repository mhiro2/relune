import {
  buildGroupListDOM,
  applyGroupVisibility,
  updateEdgeVisibility,
  syncGroupItemClass,
} from './group_toggle_dom';
import { parseReluneMetadata, type GroupMetadata } from './metadata';
import {
  emitViewerEvent,
  getViewerRuntime,
  markViewerModuleReady,
  persistDetailsOpen,
} from './viewer_api';

{
  const metadata = parseReluneMetadata();
  if (metadata) {
    const groups: GroupMetadata[] = metadata.groups ?? [];
    const groupPanel = document.getElementById('group-panel');
    const groupList = document.getElementById('group-list');

    if (groups.length === 0) {
      if (groupPanel) {
        groupPanel.style.display = 'none';
      }
    } else {
      if (groupPanel instanceof HTMLDetailsElement) {
        persistDetailsOpen(groupPanel, 'relune-group-panel-open');
      }
      const groupPanelMeta = document.getElementById('group-panel-meta');

      const groupTableMap: Record<string, string[]> = {};
      for (const group of groups) {
        groupTableMap[group.id] = group.table_ids ?? [];
      }

      const visibleGroups: Record<string, boolean> = {};
      for (const group of groups) {
        visibleGroups[group.id] = true;
      }

      /** Summarises the groups in the collapsed panel's header. */
      function syncPanelMeta(): void {
        if (groupPanelMeta === null) return;
        const hidden = groups.filter((group) => visibleGroups[group.id] === false).length;
        groupPanelMeta.textContent =
          hidden === 0 ? String(groups.length) : `${hidden} of ${groups.length} hidden`;
      }

      function isNodeHidden(nodeId: string): boolean {
        for (const groupId of Object.keys(groupTableMap)) {
          if (!visibleGroups[groupId]) {
            const tableIds = groupTableMap[groupId];
            if (tableIds?.includes(nodeId)) return true;
          }
        }
        return false;
      }

      function toggleGroup(groupId: string, visible: boolean): void {
        visibleGroups[groupId] = visible;

        const svg = document.querySelector('.canvas svg');
        if (!svg) return;

        applyGroupVisibility(svg, groupId, groupTableMap[groupId] ?? [], visible);
        updateEdgeVisibility(svg, isNodeHidden);
        syncGroupItemClass(groupId, visible);
        syncPanelMeta();

        emitViewerEvent('relune:groups-changed', {
          visibleGroups: { ...visibleGroups },
        });
      }

      function showAllGroups(): void {
        for (const group of groups) {
          const checkbox = document.getElementById(`group-${group.id}`);
          if (checkbox instanceof HTMLInputElement && !checkbox.checked) {
            checkbox.checked = true;
            toggleGroup(group.id, true);
          }
        }
      }

      function hideAllGroups(): void {
        for (const group of groups) {
          const checkbox = document.getElementById(`group-${group.id}`);
          if (checkbox instanceof HTMLInputElement && checkbox.checked) {
            checkbox.checked = false;
            toggleGroup(group.id, false);
          }
        }
      }

      const showAllBtn = document.getElementById('show-all-groups');
      const hideAllBtn = document.getElementById('hide-all-groups');
      showAllBtn?.addEventListener('click', showAllGroups);
      hideAllBtn?.addEventListener('click', hideAllGroups);

      const runtime = getViewerRuntime();
      runtime.groups = {
        setVisibility(groupId: string, visible: boolean): void {
          const checkbox = document.getElementById(`group-${groupId}`);
          if (checkbox instanceof HTMLInputElement && checkbox.checked !== visible) {
            checkbox.checked = visible;
            toggleGroup(groupId, visible);
          }
        },
        getHiddenGroups(): string[] {
          return groups
            .filter((group) => visibleGroups[group.id] === false)
            .map((group) => group.id);
        },
        isPanelOpen(): boolean {
          return groupPanel instanceof HTMLDetailsElement && groupPanel.open;
        },
        setPanelOpen(open: boolean): void {
          if (groupPanel instanceof HTMLDetailsElement) groupPanel.open = open;
        },
      };
      markViewerModuleReady('groups');

      if (groupList) {
        buildGroupListDOM(groups, groupList, toggleGroup);
      }
      syncPanelMeta();
    }
  }
}
