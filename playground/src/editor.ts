import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { PostgreSQL, sql } from "@codemirror/lang-sql";
import { HighlightStyle, bracketMatching, syntaxHighlighting } from "@codemirror/language";
import { searchKeymap } from "@codemirror/search";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap, lineNumbers } from "@codemirror/view";
import { tags } from "@lezer/highlight";

/* ------------------------------------------------------------------ */
/*  Theme                                                              */
/* ------------------------------------------------------------------ */

const reluneTheme = EditorView.theme(
  {
    "&": {
      backgroundColor: "var(--editor-bg)",
      color: "var(--editor-text)",
      fontFamily: "var(--font-mono)",
      fontSize: "12px",
      lineHeight: "1.65",
      height: "100%",
    },
    ".cm-content": {
      padding: "12px 12px 12px 4px",
      caretColor: "var(--accent)",
    },
    ".cm-cursor, .cm-dropCursor": {
      borderLeftColor: "var(--accent)",
    },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": {
      backgroundColor: "rgba(245, 158, 11, 0.2) !important",
    },
    ".cm-activeLine": {
      backgroundColor: "rgba(245, 158, 11, 0.06)",
    },
    ".cm-gutters": {
      backgroundColor: "var(--editor-bg)",
      color: "var(--text-tertiary)",
      border: "none",
      borderRight: "1px solid var(--editor-border)",
    },
    ".cm-activeLineGutter": {
      backgroundColor: "rgba(245, 158, 11, 0.06)",
      color: "var(--accent)",
    },
    ".cm-matchingBracket": {
      backgroundColor: "rgba(245, 158, 11, 0.25)",
      outline: "none",
    },
  },
  { dark: true },
);

const reluneHighlight = HighlightStyle.define([
  { tag: tags.keyword, color: "#fbbf24", fontWeight: "bold" },
  { tag: tags.typeName, color: "#d6d3d1" },
  { tag: tags.string, color: "#d97706" },
  { tag: tags.number, color: "#fcd34d" },
  { tag: tags.bool, color: "#fcd34d" },
  { tag: tags.null, color: "#fcd34d" },
  { tag: tags.operator, color: "#d6d3d1" },
  { tag: tags.punctuation, color: "#a8a29e" },
  { tag: tags.comment, color: "#78716c", fontStyle: "italic" },
  { tag: tags.variableName, color: "#fef3c7" },
  { tag: tags.definition(tags.variableName), color: "#fde68a" },
  { tag: tags.propertyName, color: "#fef3c7" },
  { tag: tags.standard(tags.name), color: "#fbbf24" },
]);

/* ------------------------------------------------------------------ */
/*  Public API                                                         */
/* ------------------------------------------------------------------ */

export type SqlEditor = {
  getValue(): string;
  setValue(text: string): void;
  onUpdate(callback: () => void): void;
  focus(): void;
};

export function createSqlEditor(parent: HTMLElement): SqlEditor {
  let changeCallback: (() => void) | undefined;

  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc: "",
      extensions: [
        lineNumbers(),
        history(),
        bracketMatching(),
        sql({ dialect: PostgreSQL }),
        reluneTheme,
        syntaxHighlighting(reluneHighlight),
        keymap.of([...defaultKeymap, ...searchKeymap, ...historyKeymap]),
        EditorView.lineWrapping,
        EditorView.contentAttributes.of({
          "aria-label": "SQL input",
          spellcheck: "false",
          autocomplete: "off",
          autocorrect: "off",
          autocapitalize: "off",
        }),
        EditorView.updateListener.of((update) => {
          if (update.docChanged && changeCallback) {
            changeCallback();
          }
        }),
      ],
    }),
  });

  return {
    getValue(): string {
      return view.state.doc.toString();
    },

    setValue(text: string): void {
      view.dispatch({
        changes: {
          from: 0,
          to: view.state.doc.length,
          insert: text,
        },
      });
    },

    onUpdate(callback: () => void): void {
      changeCallback = callback;
    },

    focus(): void {
      view.focus();
    },
  };
}
