"use strict";
(() => {
  // ts/metadata.ts
  var METADATA_ELEMENT_ID = "relune-metadata";
  function parseReluneMetadata() {
    const el = document.getElementById(METADATA_ELEMENT_ID);
    const raw = el?.textContent;
    if (raw == null || raw === "") {
      return null;
    }
    try {
      return JSON.parse(raw);
    } catch {
      return null;
    }
  }

  // ts/review_report.ts
  function sourceLabel(location) {
    return `${location.file}:${location.line}`;
  }
  function sourceKey(location) {
    return `${location.file}:${location.line}:${location.column}`;
  }
  var REVIEW_SEVERITIES = [
    "breaking",
    "caution",
    "warning",
    "info"
  ];
  var REVIEW_ELEMENT_ID = "relune-review";
  function parseReviewReport() {
    const raw = document.getElementById(REVIEW_ELEMENT_ID)?.textContent;
    if (raw == null || raw.trim() === "") return null;
    try {
      return JSON.parse(raw);
    } catch {
      return null;
    }
  }
  function findingTarget(finding) {
    const table = finding.table_name ?? null;
    if (table !== null) {
      if (finding.column_name != null) return `${table}.${finding.column_name}`;
      if (finding.fk_name != null) return `${table}.${finding.fk_name}`;
      return table;
    }
    return finding.fk_name ?? "(schema)";
  }
  function findingColumns(finding) {
    if (finding.columns !== void 0 && finding.columns.length > 0) return finding.columns;
    return finding.column_name == null ? [] : [finding.column_name];
  }
  function definitionExcerpt(lines, focus, context = 2) {
    const keep = /* @__PURE__ */ new Set();
    const anchors = new Set(focus);
    lines.forEach((line, index) => {
      if (line.change !== "unchanged") anchors.add(index);
    });
    if (lines.length > 0) keep.add(0);
    for (const anchor of anchors) {
      for (let index = anchor - context; index <= anchor + context; index++) {
        if (index >= 0 && index < lines.length) keep.add(index);
      }
    }
    const items = [];
    let gap = 0;
    lines.forEach((_line, index) => {
      if (keep.has(index)) {
        if (gap > 0) items.push({ kind: "gap", count: gap });
        gap = 0;
        items.push({ kind: "line", index });
      } else {
        gap += 1;
      }
    });
    if (gap > 0) items.push({ kind: "gap", count: gap });
    return items;
  }
  function stepFinding(visible, current, delta) {
    const count = visible.length;
    if (!visible.some(Boolean)) return null;
    let index = current ?? (delta === 1 ? -1 : count);
    for (let step = 0; step < count; step++) {
      index = (index + delta + count) % count;
      if (visible[index] === true) return index;
    }
    return null;
  }

  // ts/viewer_api.ts
  var VIEWER_RUNTIME_KEY = /* @__PURE__ */ Symbol.for("relune.viewer.runtime");
  function getViewerRuntime() {
    const viewerWindow = window;
    if (viewerWindow[VIEWER_RUNTIME_KEY] === void 0) {
      viewerWindow[VIEWER_RUNTIME_KEY] = {};
    }
    return viewerWindow[VIEWER_RUNTIME_KEY];
  }
  function noticeStack() {
    const existing = document.getElementById("relune-viewer-notices");
    if (existing instanceof HTMLElement) {
      return existing;
    }
    const stack = document.createElement("div");
    stack.id = "relune-viewer-notices";
    stack.className = "viewer-notice-stack";
    document.body.appendChild(stack);
    return stack;
  }
  function showViewerNotice(message, severity = "warning") {
    const item = document.createElement("div");
    item.className = `viewer-notice viewer-notice-${severity}`;
    item.setAttribute("role", severity === "warning" ? "alert" : "status");
    item.textContent = message;
    noticeStack().appendChild(item);
    window.setTimeout(() => {
      item.remove();
    }, 4500);
  }
  function reportSessionStorageError(action, error) {
    const isSecurityError = error instanceof DOMException && error.name === "SecurityError";
    if (isSecurityError) {
      return;
    }
    const isQuotaExceeded = error instanceof DOMException && (error.name === "QuotaExceededError" || error.name === "NS_ERROR_DOM_QUOTA_REACHED");
    if (isQuotaExceeded) {
      showViewerNotice(
        `Session storage is full while ${action}. Viewer state was not saved.`,
        "warning"
      );
      return;
    }
    console.warn(`Session storage error while ${action}`, error);
  }
  function getSessionStorage() {
    try {
      return window.sessionStorage;
    } catch (error) {
      reportSessionStorageError("accessing session storage", error);
      return null;
    }
  }
  function readSessionFlag(key) {
    const storage = getSessionStorage();
    if (storage === null) return null;
    try {
      const value = storage.getItem(key);
      return value === null ? null : value === "1";
    } catch (error) {
      reportSessionStorageError(`restoring ${key}`, error);
      return null;
    }
  }
  function writeSessionFlag(key, value) {
    const storage = getSessionStorage();
    if (storage === null) return;
    try {
      storage.setItem(key, value ? "1" : "0");
    } catch (error) {
      reportSessionStorageError(`saving ${key}`, error);
    }
  }
  function persistDetailsOpen(details, key) {
    const saved = readSessionFlag(key);
    if (saved !== null) details.open = saved;
    details.addEventListener("toggle", () => {
      writeSessionFlag(key, details.open);
    });
  }

  // ts/review_panel.ts
  {
    const report = parseReviewReport();
    const panel = document.getElementById("review-panel");
    const meta = document.getElementById("review-panel-meta");
    const dialect = document.getElementById("review-dialect");
    const coverage = document.getElementById("review-coverage");
    const severityFilter = document.getElementById("review-severity-filter");
    const list = document.getElementById("review-findings");
    const empty = document.getElementById("review-empty");
    if (report !== null && panel instanceof HTMLDetailsElement && meta instanceof HTMLElement && dialect instanceof HTMLElement && coverage instanceof HTMLElement && severityFilter instanceof HTMLElement && list instanceof HTMLElement && empty instanceof HTMLElement) {
      mountReviewPanel(report, {
        panel,
        meta,
        dialect,
        coverage,
        severityFilter,
        list,
        empty
      });
    }
  }
  function mountReviewPanel(report, elements) {
    const runtime = getViewerRuntime();
    const tables = new Map(
      (parseReluneMetadata()?.tables ?? []).map((table) => [table.id, table])
    );
    const tableLabel = (id) => tables.get(id)?.label ?? id;
    persistDetailsOpen(elements.panel, "relune-review-panel-open");
    elements.meta.textContent = summaryText(report);
    const notes = [report.dialect_note, report.migration_note].filter(
      (note) => note != null
    );
    if (notes.length > 0) {
      elements.dialect.textContent = notes.join(" \xB7 ");
      elements.dialect.removeAttribute("hidden");
    }
    const statements = new Map(
      (report.statements ?? []).map((statement) => [sourceKey(statement), statement])
    );
    renderCoverage(elements.coverage, report);
    const findings = report.findings;
    const items = findings.map(
      (finding, index) => buildFindingItem(
        finding,
        index,
        report.definitions?.[finding.table_id ?? ""],
        statements,
        tableLabel
      )
    );
    elements.list.append(...items.map((item) => item.root));
    elements.empty.toggleAttribute("hidden", findings.length > 0);
    const shown = new Set(REVIEW_SEVERITIES);
    const visible = () => findings.map((finding) => shown.has(finding.severity));
    let selected = null;
    const markSelected = (index) => {
      if (selected !== null) items[selected]?.root.classList.remove("selected");
      selected = index;
      if (index !== null) items[index]?.root.classList.add("selected");
    };
    const setExpanded = (index, expanded) => {
      const item = items[index];
      if (item === void 0) return;
      item.toggle.setAttribute("aria-expanded", String(expanded));
      item.detail.toggleAttribute("hidden", !expanded);
    };
    const selectFinding = (index, focusToggle = false) => {
      const finding = findings[index];
      const item = items[index];
      if (finding === void 0 || item === void 0) return;
      items.forEach((_item, other) => {
        if (other !== index) setExpanded(other, false);
      });
      setExpanded(index, true);
      markSelected(index);
      if (finding.table_id != null) {
        runtime.selection?.focus(finding.table_id, findingColumns(finding));
      }
      item.root.scrollIntoView?.({ block: "nearest" });
      if (focusToggle) item.toggle.focus();
    };
    items.forEach((item, index) => {
      item.toggle.addEventListener("click", () => {
        if (selected === index && item.toggle.getAttribute("aria-expanded") === "true") {
          setExpanded(index, false);
          return;
        }
        selectFinding(index);
      });
      for (const link of item.links) {
        link.button.addEventListener("click", () => {
          runtime.selection?.focus(link.tableId, link.columns);
        });
      }
    });
    document.addEventListener("relune:node-selected", (event) => {
      const nodeId = event.detail.nodeId;
      if (selected !== null && findings[selected]?.table_id !== nodeId) markSelected(null);
    });
    document.addEventListener("relune:node-cleared", () => {
      markSelected(null);
    });
    renderSeverityFilter(elements.severityFilter, report, shown, () => {
      const flags = visible();
      items.forEach((item, index) => {
        item.root.toggleAttribute("hidden", flags[index] !== true);
      });
      if (selected !== null && flags[selected] !== true) markSelected(null);
    });
    runtime.review = {
      step(delta) {
        const next = stepFinding(visible(), selected, delta);
        if (next === null) return;
        runtime.sidebar?.setCollapsed(false);
        elements.panel.open = true;
        selectFinding(next, true);
      },
      togglePanel() {
        const reveal = runtime.sidebar?.isCollapsed() === true;
        runtime.sidebar?.setCollapsed(false);
        elements.panel.open = reveal || !elements.panel.open;
      }
    };
  }
  function summaryText(report) {
    const parts = REVIEW_SEVERITIES.filter((severity) => report.summary[severity] > 0).map(
      (severity) => `${report.summary[severity]} ${severity}`
    );
    return parts.length === 0 ? "no findings" : parts.join(" \xB7 ");
  }
  function renderCoverage(container, report) {
    const notes = report.coverage_notes ?? [];
    const suppressed = report.suppressed > 0 ? [`${report.suppressed} finding(s) on excepted tables are not listed.`] : [];
    if (notes.length === 0 && suppressed.length === 0) return;
    if (notes.length > 0) {
      const heading = document.createElement("strong");
      heading.textContent = "Not fully reviewed";
      container.appendChild(heading);
    }
    const list = document.createElement("ul");
    for (const note of [...notes, ...suppressed]) {
      const item = document.createElement("li");
      item.textContent = note;
      list.appendChild(item);
    }
    container.appendChild(list);
    container.removeAttribute("hidden");
  }
  function renderSeverityFilter(container, report, shown, onChange) {
    for (const severity of REVIEW_SEVERITIES) {
      const count = report.summary[severity];
      if (count === 0) continue;
      const chip = document.createElement("button");
      chip.type = "button";
      chip.className = "review-severity-chip";
      chip.setAttribute("aria-pressed", "true");
      const dot = document.createElement("span");
      dot.className = `review-severity-dot review-severity-dot-${severity}`;
      dot.setAttribute("aria-hidden", "true");
      chip.append(dot, `${severity} ${count}`);
      chip.addEventListener("click", () => {
        const show = !shown.has(severity);
        if (show) shown.add(severity);
        else shown.delete(severity);
        chip.setAttribute("aria-pressed", String(show));
        onChange();
      });
      container.appendChild(chip);
    }
  }
  function buildFindingItem(finding, index, definition, statements, tableLabel) {
    const root = document.createElement("li");
    root.className = `review-finding review-finding-${finding.severity}`;
    const detailId = `review-finding-detail-${index}`;
    const toggle = document.createElement("button");
    toggle.type = "button";
    toggle.className = "review-finding-toggle";
    toggle.setAttribute("aria-expanded", "false");
    toggle.setAttribute("aria-controls", detailId);
    const badge = document.createElement("span");
    badge.className = `review-badge review-badge-${finding.severity}`;
    badge.textContent = finding.severity;
    const target = document.createElement("span");
    target.className = "review-finding-target";
    target.textContent = findingTarget(finding);
    const summary = document.createElement("span");
    summary.className = "review-finding-summary";
    summary.textContent = finding.rule_description;
    toggle.append(badge, target, summary);
    const detail = document.createElement("div");
    detail.className = "review-finding-detail";
    detail.id = detailId;
    detail.setAttribute("hidden", "");
    const ruleId = document.createElement("p");
    ruleId.className = "review-rule-id";
    ruleId.textContent = finding.rule_id;
    detail.append(ruleId, section("Finding", paragraph(finding.message)));
    if (finding.execution != null) {
      const execution = document.createElement("p");
      execution.className = `review-execution review-execution-${finding.execution.verdict}`;
      const verdict = document.createElement("span");
      verdict.className = "review-execution-verdict";
      verdict.textContent = finding.execution.verdict;
      execution.append(verdict, finding.execution.detail);
      detail.append(section("How the migration runs it", execution));
    }
    const sources = finding.sources ?? [];
    if (sources.length > 0) {
      detail.append(section("Migration SQL", sourceList(sources, statements)));
    }
    if (definition !== void 0 && definition.lines.length > 0) {
      detail.append(section("Definition", definitionBlock(definition, finding.focus_lines ?? [])));
    }
    const links = [];
    if (finding.table_id != null) {
      links.push(
        linkButton("table", tableLabel(finding.table_id), finding.table_id, findingColumns(finding))
      );
    }
    if (finding.related_table_id != null && finding.related_table_id !== finding.table_id) {
      links.push(
        linkButton("related", tableLabel(finding.related_table_id), finding.related_table_id, [])
      );
    }
    if (links.length > 0) {
      const row = document.createElement("div");
      row.className = "review-links";
      row.append(...links.map((link) => link.button));
      detail.append(section("Affects", row));
    }
    if (finding.mitigation != null) {
      detail.append(section("What to do", paragraph(finding.mitigation)));
    }
    root.append(toggle, detail);
    return { root, toggle, detail, links };
  }
  function sourceList(sources, statements) {
    const list = document.createElement("ol");
    list.className = "review-sources";
    for (const source of sources) {
      const item = document.createElement("li");
      const location = document.createElement("span");
      location.className = "review-source-location";
      location.textContent = sourceLabel(source);
      item.appendChild(location);
      const statement = statements.get(sourceKey(source));
      if (statement !== void 0) {
        if (statement.in_transaction) {
          const badge = document.createElement("span");
          badge.className = "review-source-transaction";
          badge.textContent = "in transaction";
          item.appendChild(badge);
        }
        const sql = document.createElement("pre");
        sql.className = "review-source-sql";
        sql.textContent = statement.sql;
        item.appendChild(sql);
      }
      list.appendChild(item);
    }
    return list;
  }
  function section(title, body) {
    const fragment = document.createDocumentFragment();
    const heading = document.createElement("h4");
    heading.textContent = title;
    fragment.append(heading, body);
    return fragment;
  }
  function paragraph(text) {
    const element = document.createElement("p");
    element.textContent = text;
    return element;
  }
  function linkButton(role, label, tableId, columns) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "review-link";
    const roleEl = document.createElement("span");
    roleEl.className = "review-link-role";
    roleEl.textContent = role;
    button.append(roleEl, label);
    return { button, tableId, columns };
  }
  function definitionBlock(definition, focus) {
    const pre = document.createElement("pre");
    pre.className = "review-definition";
    const focused = new Set(focus);
    for (const item of definitionExcerpt(definition.lines, focus)) {
      if (item.kind === "gap") {
        const gap = document.createElement("span");
        gap.className = "review-definition-gap";
        gap.textContent = `\u2026 ${item.count} unchanged line${item.count === 1 ? "" : "s"}`;
        pre.appendChild(gap);
        continue;
      }
      const line = definition.lines[item.index];
      if (line === void 0) continue;
      const span = document.createElement("span");
      const classes = ["review-definition-line", `review-definition-line-${line.change}`];
      if (focused.has(item.index)) classes.push("review-definition-line-focus");
      span.className = classes.join(" ");
      span.textContent = line.text;
      pre.appendChild(span);
    }
    return pre;
  }
})();
