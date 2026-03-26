import { parseReluneMetadata, type GroupMetadata } from './metadata';
import { emitViewerEvent } from './viewer_api';

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
        if (!groupPanel || !collapseBtn) {
          return;
        }
        groupPanel.classList.toggle('group-panel-collapsed', collapsed);
        collapseBtn.setAttribute('aria-expanded', collapsed ? 'false' : 'true');
        collapseBtn.textContent = collapsed ? '\u25B8' : '\u25BE';
      }

      collapseBtn?.addEventListener('click', () => {
        const next = !groupPanel?.classList.contains('group-panel-collapsed');
        applyPanelCollapsed(next);
        try {
          sessionStorage.setItem(COLLAPSE_KEY, next ? '1' : '0');
        } catch {
          // Ignore storage errors
        }
      });

      try {
        if (sessionStorage.getItem(COLLAPSE_KEY) === '1') {
          applyPanelCollapsed(true);
        }
      } catch {
        // Ignore storage errors
      }

      const groupTableMap: Record<string, string[]> = {};
      for (const group of groups) {
        groupTableMap[group.id] = group.table_ids ?? [];
      }

      const visibleGroups: Record<string, boolean> = {};
      for (const group of groups) {
        visibleGroups[group.id] = true;
      }

      function buildGroupList(): void {
        if (!groupList) {
          return;
        }
        groupList.innerHTML = '';

        for (const group of groups) {
          const item = document.createElement('div');
          item.className = 'group-item';
          item.setAttribute('data-group-id', group.id);

          const checkbox = document.createElement('input');
          checkbox.type = 'checkbox';
          checkbox.id = `group-${group.id}`;
          checkbox.checked = true;

          const label = document.createElement('label');
          label.setAttribute('for', `group-${group.id}`);
          label.textContent = group.label || group.id;

          const count = document.createElement('span');
          count.className = 'count';
          count.textContent = `(${group.table_ids?.length ?? 0})`;

          item.appendChild(checkbox);
          item.appendChild(label);
          item.appendChild(count);

          checkbox.addEventListener('change', () => {
            toggleGroup(group.id, checkbox.checked);
          });

          groupList.appendChild(item);
        }
      }

      function toggleGroup(groupId: string, visible: boolean): void {
        visibleGroups[groupId] = visible;

        const tableIds = groupTableMap[groupId] ?? [];
        const svg = document.querySelector('.canvas svg');
        if (!svg) {
          return;
        }

        for (const tableId of tableIds) {
          const node = svg.querySelector(`.node[data-id="${CSS.escape(tableId)}"]`);
          if (node) {
            if (visible) {
              node.classList.remove('hidden-by-group');
            } else {
              node.classList.add('hidden-by-group');
            }
          }
        }

        updateEdgeVisibility();

        const groupItem = document.querySelector(
          `.group-item[data-group-id="${CSS.escape(groupId)}"]`,
        );
        if (groupItem) {
          if (visible) {
            groupItem.classList.remove('hidden-group');
          } else {
            groupItem.classList.add('hidden-group');
          }
        }

        emitViewerEvent('relune:groups-changed', {
          visibleGroups: { ...visibleGroups },
        });
      }

      function updateEdgeVisibility(): void {
        const svg = document.querySelector('.canvas svg');
        if (!svg) {
          return;
        }

        const edges = svg.querySelectorAll('.edge');
        edges.forEach((edge) => {
          const fromId = edge.getAttribute('data-from');
          const toId = edge.getAttribute('data-to');

          const fromHidden = fromId ? isNodeHidden(fromId) : false;
          const toHidden = toId ? isNodeHidden(toId) : false;

          if (fromHidden || toHidden) {
            edge.classList.add('hidden-by-group');
          } else {
            edge.classList.remove('hidden-by-group');
          }
        });
      }

      function isNodeHidden(nodeId: string): boolean {
        for (const groupId of Object.keys(groupTableMap)) {
          if (!visibleGroups[groupId]) {
            const tableIds = groupTableMap[groupId];
            if (tableIds?.includes(nodeId)) {
              return true;
            }
          }
        }
        return false;
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

      buildGroupList();
    }
  }
}
