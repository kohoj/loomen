import { readFileSync } from "node:fs";

const main = readFileSync(new URL("../src/main.ts", import.meta.url), "utf8");
const styles = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
const distMain = readFileSync(new URL("../dist/main.js", import.meta.url), "utf8");
const distStyles = readFileSync(new URL("../dist/styles.css", import.meta.url), "utf8");

function assertIncludes(source, needle, label) {
  if (!source.includes(needle)) {
    throw new Error(`${label} missing: ${needle}`);
  }
}

function assertNotIncludes(source, needle, label) {
  if (source.includes(needle)) {
    throw new Error(`${label} still contains stale copy: ${needle}`);
  }
}

// ----------------------------------------------------------------------------
// New architecture (Direction B — efecto black + amber)
// ----------------------------------------------------------------------------

const mustHaveInMain = [
  // helpers introduced by the overhaul
  "function renderTopBar",
  "function renderBrandMark",
  "function renderLoomenMark",
  "function renderPulseHeartbeat",
  "function renderNoWorkspaceState",
  "function renderNoWorkspaceHeader",
  "function renderWorkspaceActions",
  "function renderWorkbenchTabs",
  "function renderWorkbenchComposer",
  "function renderWorkbenchInspector",
  // shell / topbar markup
  'class="shell ${workspace ? "" : "no-workspace-shell"}"',
  "${renderTopBar(repo, workspace)}",
  '<header class="topbar">',
  '<div class="brand">',
  '<div class="topbar-actions">',
  // composer beaming hook
  'class="composer ${pending && pendingSessionId ? "beaming" : ""}"',
  // workbench layout
  '<section class="workbench">',
  "${workspace ? renderWorkbenchInspector(repo, workspace) : \"\"}",
  // pulse heartbeat invocation in workspace area
  "${renderPulseHeartbeat()}",
  // empty state mark
  "loomen-mark-stack",
  "loomen-mark-tagline",
  "loomen-mark-action",
  "weave a path · seek lumen",
  // command palette + modals still wired
  "${renderCommandPalette()}",
  "${renderToolApprovalModal()}",
];
mustHaveInMain.forEach((needle) => assertIncludes(main, needle, "src/main.ts"));

const mustNotInMain = [
  // entire shader implementation gone
  "function stopLumenShader",
  "function initLumenShader",
  "lumenShaderCleanup",
  "renderLightWeave",
  "lumen-shader",
  "stained-glass-panes",
  "glass-projection",
  "lumen-aperture",
  "aperture-ray",
  "glass-came",
  "glass-pane",
  "weave-thread",
  "weave-knot",
  "weft-threads",
  "warp-threads",
  "refraction-notes",
  "thread-label",
  "spectralGlass",
  "facetCaustic",
  "wovenCaustic",
  "curtainLine",
  "lumenShaderParams",
  "__loomenShaderPause",
  "__loomenShaderResume",
  "weave-stage",
  "conductor-empty",
  "command-plate",
  "plate-primary",
  "control-strip",
  "Stained glass light threads",
  // legacy chat-pane class string gone
  'class="chat-pane',
  // legacy rail-toolbar block gone from rendered template
  'class="rail-toolbar control-strip"',
];
mustNotInMain.forEach((needle) => assertNotIncludes(main, needle, "src/main.ts"));

// ----------------------------------------------------------------------------
// Stylesheet — new token system + components
// ----------------------------------------------------------------------------

const mustHaveInStyles = [
  // self-hosted Plex fonts
  '@font-face',
  'font-family: "IBM Plex Sans"',
  'font-family: "IBM Plex Mono"',
  './fonts/ibm-plex-sans-latin-400-normal.woff2',
  './fonts/ibm-plex-mono-latin-400-normal.woff2',
  // canonical token names (the entire 12-variable system)
  '--bg: #08080a',
  '--panel: #131316',
  '--panel-2: #1c1c20',
  '--hairline: #232327',
  '--hairline-strong: #34343a',
  '--text-1: #f1f1f3',
  '--text-2: #8e8e93',
  '--text-3: #5c5c61',
  '--accent: #e8b860',
  '--accent-soft',
  '--good: #4ed4b5',
  '--danger: #d97264',
  // shell + topbar
  '.shell {',
  '.shell.no-workspace-shell',
  '.topbar {',
  '.topbar-pill',
  '.topbar-actions',
  '.brand-mark',
  // workbench
  '.workbench {',
  '.workbench-main',
  '.chat-header',
  '.composer.beaming::before',
  // pulse heartbeat
  '.pulse-heartbeat',
  '@keyframes heartbeat-run',
  '@keyframes heartbeat-fade',
  // loomen mark
  '.loomen-mark',
  '.loomen-mark-stack',
  '.loomen-mark-tagline',
  '.loomen-mark .horizon',
  '.loomen-mark .ring',
  '.loomen-mark .lumen',
  // mac drag region on top bar
  '-webkit-app-region: drag',
  // reduced-motion support
  '@media (prefers-reduced-motion: reduce)',
];
mustHaveInStyles.forEach((needle) => assertIncludes(styles, needle, "src/styles.css"));

const mustNotInStyles = [
  // legacy color tokens
  '--void-0',
  '--void-1',
  '--void-2',
  '--ink-0',
  '--ink-1',
  '--surface-1',
  '--surface-2',
  '--surface-3',
  '--surface-4',
  '--lumen-strong',
  '--ray',
  '--prism-rose',
  '--prism-violet',
  '--prism-cyan',
  '--prism-green',
  '--prism-warm',
  '--loomen-bg',
  '--loomen-panel',
  '--loomen-muted',
  '--loomen-line',
  '--loomen-ink',
  '--loomen-soft',
  '--loomen-signal',
  '--prism-amber',
  // legacy components
  '.chat-pane.no-workspace',
  '.weave-stage',
  '.command-plate',
  '.plate-primary',
  '.plate-meta',
  '.lumen-shader {',
  '.light-weave',
  '.glass-projection',
  '.stained-glass-panes',
  '.lumen-aperture',
  '.aperture-ray',
  '.refraction-notes',
  '.weave-thread',
  '.weave-knot',
  '.glass-stage',
  '.glass-controls',
  '.glass-voxel',
  '.glass-beam',
  '.lumen-track',
  '.lumen-map',
  '.map-route',
  '.loomen-verb-strip',
  '.prism-facets',
  '.holy-ray',
  // legacy fonts
  'Source Code Pro',
  // legacy filter trick
  'filter: grayscale(1) saturate(0)',
  // alternate light theme block
  '[data-theme]',
];
mustNotInStyles.forEach((needle) => assertNotIncludes(styles, needle, "src/styles.css"));

// ----------------------------------------------------------------------------
// dist/ must mirror src/ byte-for-byte
// ----------------------------------------------------------------------------

if (distMain !== main) {
  throw new Error("dist/main.js is out of sync with src/main.ts (run cp src/main.ts dist/main.js)");
}
if (distStyles !== styles) {
  throw new Error("dist/styles.css is out of sync with src/styles.css (run cp src/styles.css dist/styles.css)");
}

console.log("UI smoke check passed");
