import type { GroupMetadata } from './metadata';

export function buildGroupListDOM(
  groups: GroupMetadata[],
  container: HTMLElement,
  onChange: (groupId: string, visible: boolean) => void,
): void {
  container.innerHTML = '';

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
      onChange(group.id, checkbox.checked);
    });

    container.appendChild(item);
  }
}

export function applyGroupVisibility(svg: Element, tableIds: string[], visible: boolean): void {
  for (const tableId of tableIds) {
    const node = svg.querySelector(`.node[data-id="${CSS.escape(tableId)}"]`);
    if (node) {
      node.classList.toggle('hidden-by-group', !visible);
    }
  }
}

export function updateEdgeVisibility(
  svg: Element,
  isNodeHidden: (nodeId: string) => boolean,
): void {
  svg.querySelectorAll('.edge').forEach((edge) => {
    const fromId = edge.getAttribute('data-from');
    const toId = edge.getAttribute('data-to');
    const fromHidden = fromId ? isNodeHidden(fromId) : false;
    const toHidden = toId ? isNodeHidden(toId) : false;
    edge.classList.toggle('hidden-by-group', fromHidden || toHidden);
  });
}

export function syncGroupItemClass(groupId: string, visible: boolean): void {
  const groupItem = document.querySelector(`.group-item[data-group-id="${CSS.escape(groupId)}"]`);
  if (groupItem) {
    groupItem.classList.toggle('hidden-group', !visible);
  }
}
