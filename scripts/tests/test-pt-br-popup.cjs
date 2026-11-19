const { readFileSync } = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const assert = require("node:assert/strict");
const { test } = require("node:test");

const html = readFileSync(
  path.join(__dirname, "../../crates/codenotch/ui/notch.html"),
  "utf8",
);

function extract(start, end) {
  const i = html.indexOf(start);
  const j = html.indexOf(end, i);
  assert.ok(i >= 0 && j > i, `missing section ${start} .. ${end}`);
  return html.slice(i, j);
}

test("pt-BR UI table provides updated() formatter and stale card copy", () => {
  const source = extract("let uiLang='en';", "function setUiLanguage");
  const ctx = vm.runInNewContext(
    source + "; uiLang='pt-BR'; ({ textCopy, ui: ui() })",
  );
  assert.equal(typeof ctx.ui.updated, "function");
  assert.equal(ctx.ui.updated(ctx.ui.ago(25)), "Atualizado há 25 min");
  assert.equal(ctx.ui.usedLeft("99", "1"), "99% usado · 1% restante");
  assert.equal(
    ctx.textCopy("Last known usage — not current."),
    "Último uso conhecido — não é a leitura atual.",
  );
});

test("renderCard renders percentages and reset time for stale pt-BR Antigravity snapshot without crashing", () => {
  const pctSrc = extract("// HEADLINE_PCT_START", "// HEADLINE_PCT_END");
  const i18nSrc = extract("let uiLang='en';", "function setUiLanguage");
  const cardSrc = extract(
    "function renderCard(){",
    "// The card follows the hovered cell:",
  );

  const cardEl = { innerHTML: "", querySelector: () => null };
  let placed = false;
  let hidden = false;

  const ctx = vm.createContext({
    showRemaining: false,
    showPace: false,
    hoverId: "gemini",
    stateSnap: { sessions: [], agg: "idle", clock_24h: true },
    activity: [],
    claudeAuth: { busy: false, message: "" },
    claudeActionMessage: "",
    STATE_DOT: {
      running: "#fff",
      attention: "#ff0",
      done: "#0f8",
      idle: "#303030",
    },
    SESSION_ROWS: 5,
    WATCH: "#F2FF00",
    INK: "#ffffff",
    document: {
      getElementById: (id) => (id === "card" ? cardEl : null),
      createElement: () => ({
        set textContent(v) {
          this._v = String(v || "");
        },
        get innerHTML() {
          return this._v.replace(
            /[&<>"']/g,
            (c) =>
              ({
                "&": "&amp;",
                "<": "&lt;",
                ">": "&gt;",
                '"': "&quot;",
                "'": "&#39;",
              })[c],
          );
        },
      }),
    },
    providers: () => [
      {
        id: "gemini",
        base: "gemini",
        name: "Antigravity",
        glyph: "Ag",
        snap: {
          status: "stale",
          fetched_at: Date.now() - 25 * 60 * 1000,
          note: "",
          windows: [
            {
              id: "gemini-5h",
              label: "5-Hour Limit",
              group: "Gemini Models",
              used: 0.99,
              resets_at: Date.now() + 30 * 60 * 1000,
            },
            {
              id: "gemini-weekly",
              label: "Weekly Limit",
              group: "Gemini Models",
              used: 0.42,
              resets_at: Date.now() + 3 * 86400 * 1000,
            },
          ],
        },
      },
    ],
    glyphHtml: () => '<span class="mark">Ag</span>',
    staleOf: (snap) => snap.status === "stale",
    ago: () => "há 25 min",
    tone: (f) => (f >= 0.7 ? "#FF3F00" : "#00FF88"),
    quotaResetCopy: () => "Renova em 30 min",
    offersClaudeSignIn: () => false,
    claudeQuotaCurrent: () => true,
    moreRow: () => "",
    esc: (s) =>
      String(s || "").replace(
        /[&<>"']/g,
        (c) =>
          ({
            "&": "&amp;",
            "<": "&lt;",
            ">": "&gt;",
            '"': "&quot;",
            "'": "&#39;",
          })[c],
      ),
    placeCard: () => {
      placed = true;
    },
    hideCard: () => {
      hidden = true;
    },
  });

  vm.runInContext(
    pctSrc + "\n" + i18nSrc + "\nuiLang='pt-BR';\n" + cardSrc,
    ctx,
  );
  const ok = ctx.renderCard();

  assert.equal(ok, true);
  assert.equal(placed, true, "placeCard must run after rendering card HTML");
  assert.equal(hidden, false);
  assert.match(cardEl.innerHTML, /Uso do Antigravity/);
  assert.match(cardEl.innerHTML, /Atualizado há 25 min/);
  assert.match(cardEl.innerHTML, /class="w-pct"[^>]*>99%</);
  assert.match(cardEl.innerHTML, /99% usado · 1% restante/);
  assert.match(cardEl.innerHTML, /class="w-pct"[^>]*>42%</);
  assert.match(cardEl.innerHTML, /42% usado · 58% restante/);
});
