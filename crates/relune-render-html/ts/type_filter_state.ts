import type { TableMetadata } from './metadata';

export interface TypeFilterState {
  typeSet: Set<string>;
  allTypes: string[];
  selectedTypes: Set<string>;
  typeTableCounts: Map<string, number>;
}

export function createTypeFilterState(tables: TableMetadata[]): TypeFilterState {
  const typeSet = new Set<string>();
  const selectedTypes = new Set<string>();
  const typeTableCounts = new Map<string, number>();

  for (const table of tables) {
    const tableTypes = new Set<string>();
    for (const column of table.columns ?? []) {
      const dataType = (column.data_type ?? '').trim();
      if (dataType !== '') {
        typeSet.add(dataType);
        tableTypes.add(dataType);
      }
    }
    tableTypes.forEach((dataType) => {
      typeTableCounts.set(dataType, (typeTableCounts.get(dataType) ?? 0) + 1);
    });
  }

  const allTypes = Array.from(typeSet).sort((left, right) =>
    left.localeCompare(right, undefined, { sensitivity: 'base' }),
  );

  return { typeSet, allTypes, selectedTypes, typeTableCounts };
}

export function columnMatchesSelectedType(columnType: string, selectedType: string): boolean {
  const column = columnType.trim().toLowerCase();
  const selected = selectedType.trim().toLowerCase();
  if (column === selected) return true;

  const base = (raw: string): string => {
    const index = raw.indexOf('(');
    return (index === -1 ? raw : raw.slice(0, index)).trim();
  };

  const baseColumn = base(column);
  const baseSelected = base(selected);
  return baseColumn === baseSelected || column.includes(selected) || selected.includes(column);
}

export function tableMatchesAnySelectedType(
  table: TableMetadata,
  selectedTypes: Set<string>,
): boolean {
  return (table.columns ?? []).some((column) =>
    Array.from(selectedTypes).some((selectedType) =>
      columnMatchesSelectedType(column.data_type ?? '', selectedType),
    ),
  );
}

export function visibleTypesForQuery(allTypes: string[], query: string): string[] {
  const needle = query.trim().toLowerCase();
  if (needle === '') return allTypes;
  return allTypes.filter((dataType) => dataType.toLowerCase().includes(needle));
}

export function selectedTypeList(selectedTypes: Set<string>): string[] {
  return Array.from(selectedTypes).sort((left, right) =>
    left.localeCompare(right, undefined, { sensitivity: 'base' }),
  );
}

export function activeTypes(
  selectedTypes: Set<string>,
  allTypes: string[],
  query: string,
): string[] {
  if (selectedTypes.size > 0) return selectedTypeList(selectedTypes);
  return query.trim() === '' ? [] : visibleTypesForQuery(allTypes, query);
}
