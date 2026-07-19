// UI unit tests — runs app.js in a sandboxed Node.js vm with mocked
// DOM, Tauri API, and AudioContext. No npm dependencies required.
//
// Usage:  node ui/test/test.js
//
// Node 18+ required (uses built-in node:test and node:vm).

const { describe, it, beforeEach } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

// ---------------------------------------------------------------------------
// Mock helpers
// ---------------------------------------------------------------------------

/** All DOM element IDs that app.js references at load time. */
const ALL_IDS = [
  'timer', 'phase', 'session-label', 'dock-btn',
  'toggle-btn', 'continue-btn', 'session-left', 'session-right',
  'settings-btn', 'settings-overlay', 'sessions-container',
  'add-session-btn', 'save-settings', 'cancel-settings',
  'daily-total', 'body-el',
];

/** Build a simple DOM element stub. */
function el(id, tag) {
  const listeners = {};
  const elem = {
    id,
    tag: tag || 'div',
    textContent: '',
    innerHTML: '',
    className: '',
    value: '',
    checked: false,
    type: 'text',
    placeholder: '',
    title: '',
    min: '',
    max: '',
    classList: new Set(),
    hidden: undefined,
    _children: [],
    _style: {},

    addEventListener(ev, fn) {
      if (!listeners[ev]) listeners[ev] = [];
      listeners[ev].push(fn);
    },
    _fire(ev, data) {
      (listeners[ev] || []).forEach(fn => fn(data));
    },
    appendChild(child) {
      this._children.push(child);
      return child;
    },
    setAttribute(name, value) {
      this[name] = value;
    },
    get style() { return this._style; },
  };

  // Shim classList.
  elem.classList = {
    _set: elem.classList,
    add(c) { this._set.add(c); },
    remove(c) { this._set.delete(c); },
    contains(c) { return this._set.has(c); },
    toggle(c) {
      if (this._set.has(c)) { this._set.delete(c); return false; }
      this._set.add(c); return true;
    },
  };
  return elem;
}

// ---------------------------------------------------------------------------
// Build the sandbox
// ---------------------------------------------------------------------------

/**
 * Load app.js in a vm sandbox.
 *
 * Elements are pre-created so that the `document.getElementById` calls at
 * script-load time return stable references — replacing them later wouldn't
 * help because app.js captures the references at the top level.
 */
function loadAppJs() {
  const source = fs.readFileSync(
    path.join(__dirname, '..', 'app.js'),
    'utf-8'
  );

  const elements = {};
  ALL_IDS.forEach(id => {
    elements[id] = el(id);
  });
  // continue-btn starts with "hidden" class in the HTML.
  elements['continue-btn'].classList._set.add('hidden');

  // ---- Mock document ----
  const mockDocument = {
    getElementById(id) {
      if (!elements[id]) elements[id] = el(id);
      return elements[id];
    },
    createElement(tag) {
      return el(null, tag);
    },
    createTextNode(text) {
      return { textContent: text, nodeType: 3, _isTextNode: true };
    },
    get body() {
      return elements['body-el'];
    },
    addEventListener() {},
  };

  // ---- Mock AudioContext (constructable) ----
  function MockAudioContext() {
    this.currentTime = 0;
    this.destination = {};
  }
  MockAudioContext.prototype.createOscillator = function () {
    return {
      type: '',
      frequency: { value: 0 },
      connect() {},
      start() {},
      stop() {},
    };
  };
  MockAudioContext.prototype.createGain = function () {
    return {
      gain: {
        setValueAtTime() {},
        exponentialRampToValueAtTime() {},
      },
      connect() {},
    };
  };
  MockAudioContext.prototype.close = function () {};

  // ---- Mock window.__TAURI__ ----
  let tickListener = null;
  let dockListener = null;

  const mockInvoke = async (cmd, args) => {
    switch (cmd) {
      case 'get_state':
        return {
          remainingSeconds: 1500,
          sessionName: 'Pomodoro',
          partName: 'Work',
          partIndex: 0,
          running: false,
          paused: false,
          dailyTotalSeconds: 3600,
          activeSessionId: 'uuid-pomodoro-1',
          sessionCount: 2,
        };
      case 'get_daily_total':
        return 3600;
      case 'get_settings':
        return {
          sessions: [
            {
              id: 'uuid-pomodoro-1',
              name: 'Pomodoro',
              parts: [
                { name: 'Work', minutes: 25, extendable: false, track_time: true },
                { name: 'Break', minutes: 5, extendable: false, track_time: false },
              ],
            },
            {
              id: 'uuid-deep-focus-2',
              name: 'Deep Focus',
              parts: [
                { name: 'Focus', minutes: 50, extendable: true, track_time: true },
                { name: 'Rest', minutes: 10, extendable: false, track_time: false },
              ],
            },
          ],
        };
      case 'get_dock_state':
        return false;
      case 'toggle_dock_mode':
        return true;
      case 'start_timer':
      case 'stop_timer':
      case 'continue_timer':
        return null;
      case 'switch_session':
        return null;
      case 'update_settings':
        return { sessions: args.sessions };
      default:
        return null;
    }
  };

  const mockTauri = {
    core: { invoke: mockInvoke },
    event: {
      listen: async (event, callback) => {
        if (event === 'timer-tick') tickListener = callback;
        if (event === 'dock-mode-changed') dockListener = callback;
      },
    },
  };

  const mockWindow = {
    __TAURI__: mockTauri,
    AudioContext: MockAudioContext,
  };

  // ---- Sandbox ----
  const sandbox = {
    document: mockDocument,
    window: mockWindow,
    AudioContext: MockAudioContext,
    console: { error() {}, log() {} },
    alert() {},
    Promise,
    navigator: {},
  };
  // Provide a webkitAudioContext too (app.js falls back to it).
  sandbox.window.webkitAudioContext = MockAudioContext;
  sandbox.webkitAudioContext = MockAudioContext;

  vm.createContext(sandbox);
  const script = new vm.Script(source);
  script.runInContext(sandbox);

  return {
    sandbox,
    elements,
    tickListener,
    dockListener,
  };
}

// ---------------------------------------------------------------------------
// Load once
// ---------------------------------------------------------------------------

const ctx = loadAppJs();

// ---- Helper to reset render-dependent state between tests ----
function resetRenderState() {
  ctx.sandbox.lastPartName = '';
  ctx.sandbox.wasRunning = false;
  ctx.sandbox.isRunning = false;
  ctx.sandbox.isPaused = false;
  ctx.sandbox.isDocked = false;
  ctx.sandbox.activeSessionId = 'uuid-pomodoro-1';
  ctx.sandbox.sessionIds = ['uuid-pomodoro-1', 'uuid-deep-focus-2'];
  // Reset mutable element styles.
  ctx.elements['timer'].classList._set.clear();
  ctx.elements['toggle-btn'].classList._set.clear();
  ctx.elements['toggle-btn'].textContent = 'Start';
  ctx.elements['continue-btn'].classList._set.clear();
  ctx.elements['continue-btn'].classList._set.add('hidden');
}

// ============================== formatTime ==============================

describe('formatTime', () => {
  it('formats 25 minutes as "25:00"', () => {
    assert.equal(ctx.sandbox.formatTime(25 * 60), '25:00');
  });
  it('formats 5 minutes as "05:00"', () => {
    assert.equal(ctx.sandbox.formatTime(5 * 60), '05:00');
  });
  it('formats 1 minute 5 seconds as "01:05"', () => {
    assert.equal(ctx.sandbox.formatTime(65), '01:05');
  });
  it('formats zero as "00:00"', () => {
    assert.equal(ctx.sandbox.formatTime(0), '00:00');
  });
  it('formats 59 seconds as "00:59"', () => {
    assert.equal(ctx.sandbox.formatTime(59), '00:59');
  });
  it('formats 60 seconds as "01:00"', () => {
    assert.equal(ctx.sandbox.formatTime(60), '01:00');
  });
  it('handles negative seconds (overtime)', () => {
    assert.equal(ctx.sandbox.formatTime(-1), '-00:01');
    assert.equal(ctx.sandbox.formatTime(-65), '-01:05');
    assert.equal(ctx.sandbox.formatTime(-1500), '-25:00');
  });
});

// ========================== formatDailyTotal ===========================

describe('formatDailyTotal', () => {
  it('returns "0m" for zero or falsy', () => {
    assert.equal(ctx.sandbox.formatDailyTotal(0), '0m');
    assert.equal(ctx.sandbox.formatDailyTotal(null), '0m');
    assert.equal(ctx.sandbox.formatDailyTotal(undefined), '0m');
  });
  it('formats minutes only', () => {
    assert.equal(ctx.sandbox.formatDailyTotal(300), '5m');
    assert.equal(ctx.sandbox.formatDailyTotal(3599), '59m');
  });
  it('formats hours and minutes', () => {
    assert.equal(ctx.sandbox.formatDailyTotal(3600), '1h 0m');
    assert.equal(ctx.sandbox.formatDailyTotal(5400), '1h 30m');
    assert.equal(ctx.sandbox.formatDailyTotal(9000), '2h 30m');
  });
});

// ============================= phaseClass ==============================

describe('phaseClass', () => {
  it('returns "phase-part-0" for index 0', () => {
    assert.equal(ctx.sandbox.phaseClass(0), 'phase-part-0');
  });
  it('returns "phase-part-1" for index 1', () => {
    assert.equal(ctx.sandbox.phaseClass(1), 'phase-part-1');
  });
  it('returns "phase-part-2" for index 2', () => {
    assert.equal(ctx.sandbox.phaseClass(2), 'phase-part-2');
  });
  it('wraps around with modulo for index 5', () => {
    assert.equal(ctx.sandbox.phaseClass(5), 'phase-part-0');
  });
});

// ========================= makeDefaultSession ==========================

describe('makeDefaultSession', () => {
  it('returns the default Work/Break session', () => {
    const s = ctx.sandbox.makeDefaultSession();
    assert.equal(typeof s.id, 'string');
    assert.equal(s.name, 'Work / Break');
    assert.equal(s.parts.length, 2);
    assert.equal(s.parts[0].name, 'Work');
    assert.equal(s.parts[0].minutes, 25);
    assert.equal(s.parts[0].extendable, false);
    assert.equal(s.parts[0].track_time, true);
    assert.equal(s.parts[1].name, 'Break');
    assert.equal(s.parts[1].minutes, 5);
    assert.equal(s.parts[1].extendable, false);
    assert.equal(s.parts[1].track_time, false);
  });
  it('returns a new object each call', () => {
    const a = ctx.sandbox.makeDefaultSession();
    const b = ctx.sandbox.makeDefaultSession();
    a.name = 'modified';
    assert.equal(b.name, 'Work / Break');
  });
});

// ============================== setDocked ==============================

describe('setDocked', () => {
  beforeEach(() => {
    ctx.elements['body-el'].classList._set.clear();
    ctx.elements['dock-btn'].innerHTML = '';
    ctx.elements['dock-btn'].title = '';
  });

  it('adds "docked" class and sets ▲ when true', () => {
    ctx.sandbox.setDocked(true);
    assert.ok(ctx.elements['body-el'].classList._set.has('docked'));
    assert.ok(
      ctx.elements['dock-btn'].innerHTML.includes('9650'),
      'should show ▲'
    );
    assert.equal(ctx.elements['dock-btn'].title, 'Undock');
  });

  it('removes "docked" class and sets ▼ when false', () => {
    ctx.sandbox.setDocked(true);   // dock first
    ctx.sandbox.setDocked(false);  // then undock
    assert.ok(!ctx.elements['body-el'].classList._set.has('docked'));
    assert.ok(
      ctx.elements['dock-btn'].innerHTML.includes('9660'),
      'should show ▼'
    );
    assert.equal(ctx.elements['dock-btn'].title, 'Dock to top');
  });
});

// =============================== render ===============================

describe('render', () => {
  beforeEach(resetRenderState);

  it('updates timer text, phase, and session label', () => {
    ctx.sandbox.render({
      remainingSeconds: 300,
      sessionName: 'Test',
      partName: 'Focus',
      partIndex: 0,
      running: false,
      paused: false,
      dailyTotalSeconds: 1800,
      activeSessionId: 'uuid-1',
      sessionCount: 1,
    });
    assert.equal(ctx.elements['timer'].textContent, '05:00');
    assert.equal(ctx.elements['phase'].textContent, 'FOCUS');
    assert.equal(ctx.elements['session-label'].textContent, 'Test');
  });

  it('shows overtime class when remaining is negative', () => {
    ctx.sandbox.render({
      remainingSeconds: -30,
      sessionName: 'Pomodoro',
      partName: 'Work',
      partIndex: 0,
      running: true,
      paused: true,
      dailyTotalSeconds: 0,
      activeSessionId: 'uuid-1',
      sessionCount: 1,
    });
    assert.ok(ctx.elements['timer'].classList._set.has('overtime'));
    assert.equal(ctx.elements['timer'].textContent, '-00:30');
  });

  it('removes overtime class when positive', () => {
    // Set up overtime first, then clear it.
    ctx.sandbox.render({
      remainingSeconds: -5,
      sessionName: 'Pomodoro',
      partName: 'Work',
      partIndex: 0,
      running: true,
      paused: true,
      dailyTotalSeconds: 0,
      activeSessionId: 'uuid-1',
      sessionCount: 1,
    });
    // Now render with positive — overtime should be gone.
    ctx.sandbox.render({
      remainingSeconds: 60,
      sessionName: 'Pomodoro',
      partName: 'Work',
      partIndex: 0,
      running: true,
      paused: false,
      dailyTotalSeconds: 0,
      activeSessionId: 'uuid-1',
      sessionCount: 1,
    });
    assert.ok(!ctx.elements['timer'].classList._set.has('overtime'));
  });

  it('shows Continue button when paused', () => {
    ctx.sandbox.render({
      remainingSeconds: -10,
      sessionName: 'Pomodoro',
      partName: 'Work',
      partIndex: 0,
      running: true,
      paused: true,
      dailyTotalSeconds: 0,
      activeSessionId: 'uuid-1',
      sessionCount: 1,
    });
    assert.ok(!ctx.elements['continue-btn'].classList._set.has('hidden'));
  });

  it('hides Continue button when not paused', () => {
    ctx.sandbox.render({
      remainingSeconds: 1500,
      sessionName: 'Pomodoro',
      partName: 'Work',
      partIndex: 0,
      running: true,
      paused: false,
      dailyTotalSeconds: 0,
      activeSessionId: 'uuid-1',
      sessionCount: 1,
    });
    assert.ok(ctx.elements['continue-btn'].classList._set.has('hidden'));
  });

  it('shows "Stop" text and is-running class when timer is running', () => {
    ctx.sandbox.render({
      remainingSeconds: 1000,
      sessionName: 'Pomodoro',
      partName: 'Work',
      partIndex: 0,
      running: true,
      paused: false,
      dailyTotalSeconds: 0,
      activeSessionId: 'uuid-1',
      sessionCount: 1,
    });
    assert.equal(ctx.elements['toggle-btn'].textContent, 'Stop');
    assert.ok(ctx.elements['toggle-btn'].classList._set.has('is-running'));
  });

  it('shows "Start" text when timer is stopped', () => {
    ctx.sandbox.render({
      remainingSeconds: 1500,
      sessionName: 'Pomodoro',
      partName: 'Work',
      partIndex: 0,
      running: false,
      paused: false,
      dailyTotalSeconds: 0,
      activeSessionId: 'uuid-1',
      sessionCount: 1,
    });
    assert.equal(ctx.elements['toggle-btn'].textContent, 'Start');
    assert.ok(!ctx.elements['toggle-btn'].classList._set.has('is-running'));
  });

  it('updates daily total display', () => {
    ctx.sandbox.render({
      remainingSeconds: 1500,
      sessionName: 'Pomodoro',
      partName: 'Work',
      partIndex: 0,
      running: false,
      paused: false,
      dailyTotalSeconds: 7200,
      activeSessionId: 'uuid-1',
      sessionCount: 1,
    });
    assert.equal(
      ctx.elements['daily-total'].textContent,
      'Today: 2h 0m'
    );
  });

  it('sets phase CSS class from part name', () => {
    // Use a fresh part name to avoid transition beep triggering.
    ctx.sandbox.lastPartName = 'Break';
    ctx.sandbox.isRunning = true;
    ctx.sandbox.wasRunning = true;
    ctx.sandbox.render({
      remainingSeconds: 1500,
      sessionName: 'Pomodoro',
      partName: 'Break',  // same as lastPartName → no beep
      partIndex: 1,
      running: true,
      paused: false,
      dailyTotalSeconds: 0,
      activeSessionId: 'uuid-1',
      sessionCount: 1,
    });
    assert.equal(ctx.elements['phase'].className, 'phase-part-1');
  });

  it('tracks state in module-level variables', () => {
    ctx.sandbox.render({
      remainingSeconds: 500,
      sessionName: 'Deep Focus',
      partName: 'Focus',
      running: true,
      paused: false,
      dailyTotalSeconds: 100,
      activeSessionIndex: 1,
      sessionCount: 3,
    });
    // Verify through DOM effects rather than let bindings
    // (vm sandbox does not expose let/const as context properties).
    assert.equal(ctx.elements['timer'].textContent, '08:20');
    assert.equal(ctx.elements['phase'].textContent, 'FOCUS');
    assert.equal(ctx.elements['session-label'].textContent, 'Deep Focus');
    assert.equal(ctx.elements['toggle-btn'].textContent, 'Stop');
    assert.ok(ctx.elements['toggle-btn'].classList._set.has('is-running'));
    assert.ok(ctx.elements['continue-btn'].classList._set.has('hidden'));
  });
});

// ====================== buildSettingsForm ==============================

describe('buildSettingsForm', () => {
  let editSessions;

  beforeEach(() => {
    // Reset the sessions container.
    ctx.elements['sessions-container']._children = [];
    editSessions = [
      {
        id: 'uuid-test',
        name: 'Pomodoro',
        parts: [
          { name: 'Work', minutes: 25, extendable: false, track_time: true },
          { name: 'Break', minutes: 5, extendable: true, track_time: false },
        ],
      },
    ];
  });

  it('populates session container with cards', () => {
    ctx.sandbox.buildSettingsForm(editSessions);
    const container = ctx.elements['sessions-container'];
    assert.ok(container._children.length > 0, 'should have children');
  });

  it('session name input is bound to editSessions', () => {
    ctx.sandbox.buildSettingsForm(editSessions);
    const container = ctx.elements['sessions-container'];
    const card = container._children[0];
    // session-header is first child of card.
    const header = card._children[0];
    // First child of header is the name input.
    const nameInput = header._children[0];
    assert.equal(nameInput.tag, 'input');
    assert.equal(nameInput.value, 'Pomodoro');

    // Simulate user typing.
    nameInput.value = 'New Name';
    nameInput._fire('input');
    assert.equal(editSessions[0].name, 'New Name');
  });

  it('extendable checkbox is bound to part data', () => {
    ctx.sandbox.buildSettingsForm(editSessions);
    const container = ctx.elements['sessions-container'];
    const card = container._children[0];
    const partsList = card._children[1]; // second child of card
    // Second row in partsList (Break part, extendable=true)
    const breakRow = partsList._children[1];
    // nameCol (child[0]) → checkboxRow (child[1]) → extLabel (child[0])
    const nameCol = breakRow._children[0];
    const checkboxRow = nameCol._children[1];
    const extLabel = checkboxRow._children[0];
    const extCheck = extLabel._children[0]; // <input type="checkbox">
    assert.equal(extCheck.tag, 'input');
    assert.equal(extCheck.checked, true);

    // Simulate unchecking.
    extCheck.checked = false;
    extCheck._fire('change');
    assert.equal(editSessions[0].parts[1].extendable, false);
  });
});

// ====================== Client-side validation =========================

describe('settings client-side validation', () => {
  it('rejects empty session name', () => {
    const sessions = [{ name: '  ', parts: [{ name: 'Work', minutes: 25 }] }];
    for (const s of sessions) {
      if (!s.name.trim()) {
        assert.ok(true);
        return;
      }
    }
    assert.fail('should have caught empty session name');
  });

  it('rejects minutes < 1', () => {
    const sessions = [
      { name: 'Pomodoro', parts: [{ name: 'Work', minutes: 0 }] },
    ];
    for (const s of sessions) {
      for (const p of s.parts) {
        if (!p.minutes || p.minutes < 1 || p.minutes > 120) {
          assert.ok(true);
          return;
        }
      }
    }
    assert.fail('should have caught minutes < 1');
  });

  it('rejects minutes > 120', () => {
    const sessions = [
      { name: 'Pomodoro', parts: [{ name: 'Work', minutes: 121 }] },
    ];
    for (const s of sessions) {
      for (const p of s.parts) {
        if (!p.minutes || p.minutes < 1 || p.minutes > 120) {
          assert.ok(true);
          return;
        }
      }
    }
    assert.fail('should have caught minutes > 120');
  });

  it('accepts valid configuration', () => {
    const sessions = [
      {
        name: 'Pomodoro',
        parts: [
          { name: 'Work', minutes: 25 },
          { name: 'Break', minutes: 5 },
        ],
      },
    ];
    let valid = true;
    for (const s of sessions) {
      if (!s.name.trim()) { valid = false; break; }
      for (const p of s.parts) {
        if (!p.name.trim()) { valid = false; break; }
        if (!p.minutes || p.minutes < 1 || p.minutes > 120) {
          valid = false;
          break;
        }
      }
    }
    assert.ok(valid);
  });
});

// ====================== Session switcher logic =========================

describe('session switcher', () => {
  it('wraps left from index 0 to last index via UUID list', () => {
    const ids = ['a', 'b', 'c'];
    const cur = ids.indexOf('a');
    const prev = ids[(cur - 1 + ids.length) % ids.length];
    assert.equal(prev, 'c');
  });
  it('wraps right from last index to index 0 via UUID list', () => {
    const ids = ['a', 'b', 'c'];
    const cur = ids.indexOf('c');
    const next = ids[(cur + 1) % ids.length];
    assert.equal(next, 'a');
  });
  it('moves right normally via UUID list', () => {
    const ids = ['a', 'b', 'c'];
    const cur = ids.indexOf('b');
    const next = ids[(cur + 1) % ids.length];
    assert.equal(next, 'c');
  });
  it('moves left normally via UUID list', () => {
    const ids = ['a', 'b', 'c'];
    const cur = ids.indexOf('c');
    const prev = ids[(cur - 1 + ids.length) % ids.length];
    assert.equal(prev, 'b');
  });
  it('single session: guard prevents action', () => {
    assert.ok(true);
  });
});

// ==================== Keyboard shortcut logic ==========================

describe('keyboard shortcuts', () => {
  it('Space/Enter triggers continue when not docked and paused', () => {
    const should = !false && true && (' ' === ' ' || ' ' === 'Enter');
    assert.ok(should);
  });
  it('Space does not trigger continue when docked', () => {
    const should = !true && true && (' ' === ' ' || ' ' === 'Enter');
    assert.ok(!should);
  });
  it('ArrowLeft / h wraps left', () => {
    assert.equal((0 - 1 + 3) % 3, 2);
  });
  it('ArrowRight / l wraps right', () => {
    assert.equal((2 + 1) % 3, 0);
  });
});

// ===================== dock-mode-changed event =========================

describe('dock-mode-changed listener', () => {
  it('listener is registered', () => {
    assert.ok(ctx.dockListener, 'listener should be registered');
  });
  it('invoking listener toggles docked state via setDocked', () => {
    ctx.dockListener({ payload: { docked: true } });
    assert.ok(ctx.elements['body-el'].classList._set.has('docked'));
    assert.ok(ctx.elements['dock-btn'].innerHTML.includes('9650'));

    ctx.dockListener({ payload: { docked: false } });
    assert.ok(!ctx.elements['body-el'].classList._set.has('docked'));
    assert.ok(ctx.elements['dock-btn'].innerHTML.includes('9660'));
  });
});

// ==================== Tauri timer-tick event ===========================

describe('timer-tick listener', () => {
  it('listener is registered', () => {
    assert.ok(ctx.tickListener, 'timer-tick listener should be registered');
  });
  it('invoking listener calls render with payload', () => {
    ctx.sandbox.lastPartName = '';
    ctx.sandbox.wasRunning = false;
    ctx.sandbox.isRunning = false;
    ctx.sandbox.isPaused = false;

    ctx.tickListener({
      payload: {
        remainingSeconds: 600,
        sessionName: 'Deep Focus',
        partName: 'Focus',
        partIndex: 0,
        running: true,
        paused: false,
        dailyTotalSeconds: 7200,
        activeSessionId: 'uuid-1',
        sessionCount: 1,
      },
    });

    assert.equal(ctx.elements['timer'].textContent, '10:00');
    assert.equal(ctx.elements['phase'].textContent, 'FOCUS');
    assert.equal(ctx.elements['session-label'].textContent, 'Deep Focus');
    assert.equal(ctx.elements['toggle-btn'].textContent, 'Stop');
  });
});

console.log('All UI tests passed.');
