import { parseReluneMetadata, type GroupMetadata } from './metadata';
import {
  emitViewerEvent,
  getViewerRuntime,
  markViewerModuleReady,
  reportSessionStorageError,
} from './viewer_api';
import {
  buildGroupListDOM,
  applyGroupVisibility,
  updateEdgeVisibility,
  syncGroupItemClass,
} from './group_toggle_dom';

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
      const collapseBtn = document.getElementById('group-panel-collapse');
      const COLLAPSE_KEY = 'relune-group-panel-collapsed';

      function applyPanelCollapsed(collapsed: boolean): void {
        if (!groupPanel || !collapseBtn) return;
        groupPanel.classList.toggle('group-panel-collapsed', collapsed);
        collapseBtn.setAttribute('aria-expanded', collapsed ? 'false' : 'true');
        collapseBtn.textContent = collapsed ? '\u25B8' : '\u25BE';
      }

      collapseBtn?.addEventListener('click', () => {
        const next = !groupPanel?.classList.contains('group-panel-collapsed');
        applyPanelCollapsed(next);
        try {
          sessionStorage.setItem(COLLAPSE_KEY, next ? '1' : '0');
        } catch (error: unknown) {
          reportSessionStorageError('saving the group panel state', error);
        }
      });

      try {
        if (sessionStorage.getItem(COLLAPSE_KEY) === '1') {
          applyPanelCollapsed(true);
        }
      } catch (error: unknown) {
        reportSessionStorageError('restoring the group panel state', error);
      }

      const groupTableMap: Record<string, string[]> = {};
      for (const group of groups) {
        groupTableMap[group.id] = group.table_ids ?? [];
      }

      const visibleGroups: Record<string, boolean> = {};
      for (const group of groups) {
        visibleGroups[group.id] = true;
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

        applyGroupVisibility(svg, groupTableMap[groupId] ?? [], visible);
        updateEdgeVisibility(svg, isNodeHidden);
        syncGroupItemClass(groupId, visible);

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
      };
      markViewerModuleReady('groups');

      if (groupList) {
        buildGroupListDOM(groups, groupList, toggleGroup);
      }
    }
  }
}
