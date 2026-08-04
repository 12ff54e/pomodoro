// ---- State (mirrored from Rust, updated by events) ----
let sessions = [];
let activeSessionId = '';
let currentPartName = 'Work';
let currentPartIndex = 0;
let currentSessionName = 'Pomodoro';
let sessionCount = 1;
let sessionIds = [];
let isRunning = false;
let isPaused = false;
let isManualPause = false;
let partCount = 1;
let skipNextBeep = false;
let wasRunning = false;
let lastPartName = '';
let isDocked = false;
let shortcutConfig = { toggle: 'CmdOrCtrl+Shift+S', pause: '', nextPart: 'CmdOrCtrl+Shift+C' };

// ---- Audio context (lazy, created on first beep) ----
let audioCtx = null;
function getAudioCtx() {
  if (!audioCtx) {
    audioCtx = new (window.AudioContext || window.webkitAudioContext)();
  }
  return audioCtx;
}

/** Play a simple beep: frequency in Hz, duration in ms, repeat count. */
function beep(freq, durationMs, count = 1) {
  const ctx = getAudioCtx();
  let delay = 0;
  for (let i = 0; i < count; i++) {
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.type = 'sine';
    osc.frequency.value = freq;
    gain.gain.setValueAtTime(0.3, ctx.currentTime + delay);
    gain.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + delay + durationMs / 1000);
    osc.connect(gain);
    gain.connect(ctx.destination);
    osc.start(ctx.currentTime + delay);
    osc.stop(ctx.currentTime + delay + durationMs / 1000 + 0.05);
    delay += durationMs / 1000 + 0.15;
  }
}

// ---- DOM references ----
const timerEl = document.getElementById('timer');
const phaseEl = document.getElementById('phase');
const sessionLabelEl = document.getElementById('session-label');
const dockBtn = document.getElementById('dock-btn');
const toggleBtn = document.getElementById('toggle-btn');
const pauseBtn = document.getElementById('pause-btn');
const nextBtn = document.getElementById('next-btn');
const sessionLeftBtn = document.getElementById('session-left');
const sessionRightBtn = document.getElementById('session-right');
const settingsBtn = document.getElementById('settings-btn');
const overlay = document.getElementById('settings-overlay');
const sessionsContainer = document.getElementById('sessions-container');
const addSessionBtn = document.getElementById('add-session-btn');
const exportBtn = document.getElementById('export-settings');
const importBtn = document.getElementById('import-settings');
const saveBtn = document.getElementById('save-settings');
const cancelBtn = document.getElementById('cancel-settings');
const dailyTotalEl = document.getElementById('daily-total');
const tabShortcuts = document.getElementById('tab-shortcuts');

// ---- Helpers ----
function formatTime(totalSeconds) {
  const abs = Math.abs(totalSeconds);
  const m = Math.floor(abs / 60);
  const s = abs % 60;
  const sign = totalSeconds < 0 ? '-' : '';
  return sign + String(m).padStart(2, '0') + ':' + String(s).padStart(2, '0');
}

function formatDailyTotal(totalSeconds) {
  if (!totalSeconds || totalSeconds === 0) return '0m';
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  if (hours > 0) return hours + 'h ' + minutes + 'm';
  return minutes + 'm';
}

/** Map a part index to a CSS class for the phase badge. */
const PART_COLORS = 5;
function phaseClass(partIndex) {
  return 'phase-part-' + (partIndex % PART_COLORS);
}

function isLastPart() {
  return currentPartIndex >= partCount - 1;
}

function render(tick) {
  // Beep on timer-driven transitions only (not manual switches or startup).
  const partChanged = tick.partName !== lastPartName;
  const wasSkip = skipNextBeep;
  skipNextBeep = false;
  if (tick.running && !wasRunning) {
    // Session started (user clicked Start) — single long beep.
    beep(660, 600, 1);
  } else if (partChanged && tick.running && !tick.paused && !isPaused) {
    // Timer auto-advanced to the next part — short triple beep (suppressed on manual skip).
    if (!wasSkip) beep(880, 150, 3);
  } else if (partChanged && !tick.running && wasRunning) {
    // Session finished (last part ended, timer stopped) — single long beep (suppressed on manual skip).
    if (!wasSkip) beep(660, 600, 1);
  } else if (tick.paused && !isPaused) {
    // Just entered overtime — same triple beep as normal transitions.
    beep(880, 150, 3);
  }
  if (partChanged) {
    lastPartName = tick.partName;
  }
  wasRunning = tick.running;

  currentPartName = tick.partName;
  currentPartIndex = tick.partIndex;
  currentSessionName = tick.sessionName;
  activeSessionId = tick.activeSessionId;
  sessionCount = tick.sessionCount;
  isRunning = tick.running;
  isPaused = tick.paused;
  isManualPause = tick.manualPause;
  partCount = tick.partCount || 1;

  timerEl.textContent = formatTime(tick.remainingSeconds);

  // Add overtime class when in negative time.
  if (tick.remainingSeconds < 0) {
    timerEl.classList.add('overtime');
  } else {
    timerEl.classList.remove('overtime');
  }

  phaseEl.textContent = tick.partName.toUpperCase();
  phaseEl.className = phaseClass(tick.partIndex);
  sessionLabelEl.textContent = tick.sessionName;

  // ---- Button visibility ----
  if (tick.running) {
    toggleBtn.textContent = 'Stop';
    toggleBtn.classList.add('is-running');

    // Pause/Continue button: visible whenever running.
    pauseBtn.classList.remove('hidden');
    if (tick.manualPause) {
      pauseBtn.textContent = 'Continue';
    } else {
      pauseBtn.textContent = 'Pause';
    }

    // Next Part button: visible when running and not on last part.
    if (isLastPart()) {
      nextBtn.classList.add('hidden');
    } else {
      nextBtn.classList.remove('hidden');
    }
  } else {
    toggleBtn.textContent = 'Start';
    toggleBtn.classList.remove('is-running');
    pauseBtn.classList.add('hidden');
    nextBtn.classList.add('hidden');
  }

  if (tick.dailyTotalSeconds !== undefined) {
    dailyTotalEl.textContent = 'Today: ' + formatDailyTotal(tick.dailyTotalSeconds);
  }
}

// ---- Tauri API ----
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

listen('timer-tick', (event) => {
  render(event.payload);
});

listen('dock-mode-changed', (event) => {
  setDocked(event.payload.docked);
});

/** Update UI for current dock state. */
function setDocked(docked) {
  isDocked = docked;
  if (docked) {
    document.body.classList.add('docked');
    dockBtn.innerHTML = '&#9650;';  // ▲  up arrow = undock
    dockBtn.title = 'Undock';
  } else {
    document.body.classList.remove('docked');
    dockBtn.innerHTML = '&#9660;';  // ▼  down arrow = dock
    dockBtn.title = 'Dock to top';
  }
}

// ---- Dock button ----
dockBtn.addEventListener('click', async () => {
  try {
    await invoke('toggle_dock_mode');
    // setDocked() is called by the 'dock-mode-changed' event listener.
  } catch (e) {
    console.error('toggle_dock_mode failed:', e);
  }
});

// ---- Toggle button ----
toggleBtn.addEventListener('click', async () => {
  try {
    if (isRunning) {
      await invoke('stop_timer');
    } else {
      await invoke('start_timer');
    }
  } catch (e) {
    console.error('toggle failed:', e);
  }
});

// ---- Pause / Continue button ----
pauseBtn.addEventListener('click', async () => {
  try {
    if (isManualPause) {
      await invoke('resume_timer');
    } else {
      await invoke('pause_timer');
    }
  } catch (e) {
    console.error('pause/resume failed:', e);
  }
});

// ---- Next Part button ----
nextBtn.addEventListener('click', async () => {
  try {
    skipNextBeep = true;
    await invoke('next_part');
  } catch (e) {
    skipNextBeep = false;
    console.error('next_part failed:', e);
  }
});

// ---- Session switcher arrows ----
sessionLeftBtn.addEventListener('click', async () => {
  if (isRunning || sessionCount <= 1) return;
  const cur = sessionIds.indexOf(activeSessionId);
  if (cur < 0) return;
  const id = sessionIds[(cur - 1 + sessionIds.length) % sessionIds.length];
  try {
    await invoke('switch_session', { sessionId: id });
  } catch (e) {
    console.error('switch_session failed:', e);
  }
});

sessionRightBtn.addEventListener('click', async () => {
  if (isRunning || sessionCount <= 1) return;
  const cur = sessionIds.indexOf(activeSessionId);
  if (cur < 0) return;
  const id = sessionIds[(cur + 1) % sessionIds.length];
  try {
    await invoke('switch_session', { sessionId: id });
  } catch (e) {
    console.error('switch_session failed:', e);
  }
});

// ---- Keyboard shortcuts ----
document.addEventListener('keydown', async (e) => {
  if (e.target.tagName === 'INPUT') return;

  // Escape closes the settings panel if it's open.
  if (e.key === 'Escape' && !overlay.classList.contains('hidden')) {
    closeSettings();
    return;
  }

  // Space/Enter to advance to next part when running — not in dock mode.
  if (!isDocked && isRunning && (e.key === ' ' || e.key === 'Enter')) {
    e.preventDefault();
    try { await invoke('next_part'); } catch (_) {}
    return;
  }

  if (isRunning || sessionCount <= 1) return;

  const cur = sessionIds.indexOf(activeSessionId);
  if (cur < 0) return;

  if (e.key === 'ArrowLeft' || e.key === 'h') {
    const id = sessionIds[(cur - 1 + sessionIds.length) % sessionIds.length];
    try { await invoke('switch_session', { sessionId: id }); } catch (_) {}
  } else if (e.key === 'ArrowRight' || e.key === 'l') {
    const id = sessionIds[(cur + 1) % sessionIds.length];
    try { await invoke('switch_session', { sessionId: id }); } catch (_) {}
  }
});

// ---- Dynamic settings form ----

/** Create a new default session (Work → Break). */
function makeDefaultSession() {
  return {
    id: '',   // Server assigns UUID on save
    name: 'Work / Break',
    parts: [
      { name: 'Work', minutes: 25, extendable: false, track_time: true },
      { name: 'Break', minutes: 5, extendable: false, track_time: false },
    ],
  };
}

/** Rebuild the settings form from the sessions array. */
function buildSettingsForm(editSessions) {
  sessionsContainer.innerHTML = '';

  editSessions.forEach((session, si) => {
    const card = document.createElement('div');
    card.className = 'session-card';

    // Header: session name + delete button.
    const header = document.createElement('div');
    header.className = 'session-header';

    const nameInput = document.createElement('input');
    nameInput.type = 'text';
    nameInput.value = session.name;
    nameInput.placeholder = 'Session name';
    nameInput.addEventListener('input', () => {
      editSessions[si].name = nameInput.value;
    });

    const delBtn = document.createElement('button');
    delBtn.className = 'btn-delete-session';
    delBtn.textContent = '×';  // ×
    delBtn.title = 'Delete session';
    delBtn.addEventListener('click', () => {
      if (editSessions.length <= 1) return; // keep at least 1 session
      editSessions.splice(si, 1);
      buildSettingsForm(editSessions);
    });

    header.appendChild(nameInput);
    header.appendChild(delBtn);

    // Parts list.
    const partsList = document.createElement('div');
    partsList.className = 'parts-list';

    session.parts.forEach((part, pi) => {
      const row = document.createElement('div');
      row.className = 'part-row';

      const nameCol = document.createElement('div');
      nameCol.className = 'part-name-col';

      const partName = document.createElement('input');
      partName.type = 'text';
      partName.value = part.name;
      partName.placeholder = 'Part name';
      partName.addEventListener('input', () => {
        editSessions[si].parts[pi].name = partName.value;
      });

      const checkboxRow = document.createElement('div');
      checkboxRow.className = 'checkbox-row';

      const extLabel = document.createElement('label');
      extLabel.className = 'extendable-label';
      const extCheck = document.createElement('input');
      extCheck.type = 'checkbox';
      extCheck.checked = part.extendable || false;
      extCheck.title = 'Extendable: timer continues past zero until you click Continue';
      extCheck.addEventListener('change', () => {
        editSessions[si].parts[pi].extendable = extCheck.checked;
      });
      extLabel.appendChild(extCheck);
      extLabel.appendChild(document.createTextNode(' Ext'));

      const trackLabel = document.createElement('label');
      trackLabel.className = 'extendable-label';
      const trackCheck = document.createElement('input');
      trackCheck.type = 'checkbox';
      trackCheck.checked = part.track_time || false;
      trackCheck.title = 'Track time: record seconds spent on this part to the daily log';
      trackCheck.addEventListener('change', () => {
        editSessions[si].parts[pi].track_time = trackCheck.checked;
      });
      trackLabel.appendChild(trackCheck);
      trackLabel.appendChild(document.createTextNode(' Track'));

      checkboxRow.appendChild(extLabel);
      checkboxRow.appendChild(trackLabel);

      nameCol.appendChild(partName);
      nameCol.appendChild(checkboxRow);

      const partMin = document.createElement('input');
      partMin.type = 'number';
      partMin.min = 1;
      partMin.max = 120;
      partMin.value = part.minutes;
      partMin.addEventListener('input', () => {
        const v = parseInt(partMin.value, 10);
        if (!isNaN(v)) editSessions[si].parts[pi].minutes = v;
      });

      const rmBtn = document.createElement('button');
      rmBtn.className = 'btn-delete-part';
      rmBtn.textContent = '×';
      rmBtn.title = 'Remove part';
      rmBtn.addEventListener('click', () => {
        if (editSessions[si].parts.length <= 1) return;
        editSessions[si].parts.splice(pi, 1);
        buildSettingsForm(editSessions);
      });

      row.appendChild(nameCol);
      row.appendChild(partMin);
      row.appendChild(rmBtn);
      partsList.appendChild(row);
    });

    // Add part button.
    const addPart = document.createElement('button');
    addPart.className = 'btn-add-part';
    addPart.textContent = '+ Add Part';
    addPart.addEventListener('click', () => {
      editSessions[si].parts.push({ name: 'Rest', minutes: 5, extendable: false, track_time: false });
      buildSettingsForm(editSessions);
    });

    card.appendChild(header);
    card.appendChild(partsList);
    card.appendChild(addPart);
    sessionsContainer.appendChild(card);
  });
}

/** Build the shortcuts editing form. */
function buildShortcutsForm(editShortcuts) {
  tabShortcuts.innerHTML = '';

  const display = (s) => s ? s.replace(/^CmdOrCtrl/, 'Ctrl/Cmd').replace(/Key|Digit/g, '') : '';

  const actions = [
    { key: 'toggle', label: 'Toggle Start/Stop' },
    { key: 'pause', label: 'Toggle Pause/Resume' },
    { key: 'nextPart', label: 'Next Part' },
  ];

  actions.forEach(({ key, label }) => {
    const row = document.createElement('div');
    row.className = 'shortcut-row';

    const lbl = document.createElement('span');
    lbl.className = 'shortcut-label';
    lbl.textContent = label;

    const keyBtn = document.createElement('button');
    keyBtn.className = 'shortcut-key';
    keyBtn.textContent = editShortcuts[key] ? display(editShortcuts[key]) : '—';
    if (!editShortcuts[key]) {
      keyBtn.classList.add('empty');
    }

    // Click to start recording a new shortcut.
    keyBtn.addEventListener('click', () => {
      let cancelled = false;
      const previous = editShortcuts[key];
      keyBtn.textContent = 'Press keys…';
      keyBtn.classList.add('recording');
      keyBtn.classList.remove('empty');

      function onKeyDown(e) {
        e.preventDefault();
        e.stopPropagation();

        if (e.key === 'Escape') {
          cancelled = true;
          cleanup();
          editShortcuts[key] = previous;
          keyBtn.textContent = previous ? display(previous) : '—';
          if (!previous) keyBtn.classList.add('empty');
          keyBtn.classList.remove('recording');
          return;
        }

        // Build the shortcut string from modifiers + physical key code.
        const modifiers = [];
        if (e.ctrlKey || e.metaKey) modifiers.push('CmdOrCtrl');
        if (e.shiftKey) modifiers.push('Shift');
        if (e.altKey) modifiers.push('Alt');

        // Only accept keys that produce a physical code.
        if (!e.code || (!e.code.startsWith('Key') && !e.code.startsWith('Digit') && e.code !== 'Space')) {
          return;
        }
        if (modifiers.length === 0) return;

        const shortcut = [...modifiers, e.code].join('+');
        cleanup();
        editShortcuts[key] = shortcut;
        keyBtn.textContent = display(shortcut);
        keyBtn.classList.remove('recording');
      }

      function cleanup() {
        document.removeEventListener('keydown', onKeyDown, true);
        document.removeEventListener('mousedown', onMouseDown, true);
      }

      function onMouseDown(e) {
        // Clicking outside cancels recording.
        if (!keyBtn.contains(e.target)) {
          cancelled = true;
          cleanup();
          editShortcuts[key] = previous;
          keyBtn.textContent = previous ? display(previous) : '—';
          if (!previous) keyBtn.classList.add('empty');
          keyBtn.classList.remove('recording');
        }
      }

      // Use capture phase to intercept before browser/OS.
      document.addEventListener('keydown', onKeyDown, true);
      // Slight delay so this click doesn't immediately cancel itself.
      setTimeout(() => {
        document.addEventListener('mousedown', onMouseDown, true);
      }, 0);
    });

    // Clear button.
    const clearBtn = document.createElement('button');
    clearBtn.className = 'btn-clear-shortcut';
    clearBtn.textContent = '×';  // ×
    clearBtn.title = 'Clear shortcut';
    clearBtn.addEventListener('click', () => {
      editShortcuts[key] = '';
      keyBtn.textContent = '—';
      keyBtn.classList.add('empty');
    });

    row.appendChild(lbl);
    row.appendChild(keyBtn);
    row.appendChild(clearBtn);
    tabShortcuts.appendChild(row);
  });
}

// ---- Settings open / close ----
settingsBtn.addEventListener('click', () => {
  // Deep-clone sessions for editing.
  const editSessions = sessions.map(s => ({
    id: s.id,
    name: s.name,
    parts: s.parts.map(p => ({
      name: p.name, minutes: p.minutes,
      extendable: p.extendable || false,
      track_time: p.track_time || false,
    })),
  }));
  buildSettingsForm(editSessions);

  // Deep-clone shortcuts for editing.
  const editShortcuts = {
    toggle: shortcutConfig.toggle || '',
    pause: shortcutConfig.pause || '',
    nextPart: shortcutConfig.nextPart || '',
  };
  buildShortcutsForm(editShortcuts);

  // Store references for save.
  overlay._editSessions = editSessions;
  overlay._editShortcuts = editShortcuts;

  // Reset to first tab.
  switchSettingsTab('sessions');

  overlay.classList.remove('hidden');
  invoke('set_settings_open', { open: true }).catch(() => {});
});

// ---- Tab switching ----
document.querySelectorAll('.settings-tab').forEach(tab => {
  tab.addEventListener('click', () => {
    switchSettingsTab(tab.dataset.tab);
  });
});

function switchSettingsTab(tabName) {
  document.querySelectorAll('.settings-tab').forEach(t => {
    t.classList.toggle('active', t.dataset.tab === tabName);
  });
  document.querySelectorAll('.tab-content').forEach(c => {
    c.classList.toggle('hidden', c.id !== 'tab-' + tabName);
  });
}

function closeSettings() {
  overlay.classList.add('hidden');
  invoke('set_settings_open', { open: false }).catch(() => {});
}

cancelBtn.addEventListener('click', () => {
  closeSettings();
});

addSessionBtn.addEventListener('click', () => {
  const edit = overlay._editSessions;
  if (!edit) return;
  edit.push(makeDefaultSession());
  buildSettingsForm(edit);
});

saveBtn.addEventListener('click', async () => {
  const edit = overlay._editSessions;
  if (!edit) return;

  // Basic client-side validation.
  for (const s of edit) {
    if (!s.name.trim()) {
      alert('Each session must have a name.');
      return;
    }
    for (const p of s.parts) {
      if (!p.minutes || p.minutes < 1 || p.minutes > 120) {
        alert('Part minutes must be between 1 and 120.');
        return;
      }
    }
  }

  try {
    const editShortcuts = overlay._editShortcuts;
    const newSettings = await invoke('update_settings', {
      sessions: edit,
      shortcuts: editShortcuts
        ? {
            toggle: editShortcuts.toggle,
            pause: editShortcuts.pause,
            next_part: editShortcuts.nextPart,
          }
        : undefined,
    });
    sessions = newSettings.sessions;
    sessionIds = newSettings.sessions.map(s => s.id);
    if (newSettings.shortcuts) {
      shortcutConfig = newSettings.shortcuts;
    }
    closeSettings();
  } catch (e) {
    console.error('update_settings failed:', e);
    alert('Failed to save: ' + e);
  }
});

// ---- Export settings to clipboard ----
exportBtn.addEventListener('click', async () => {
  try {
    const settings = await invoke('get_settings');
    const json = JSON.stringify(settings, null, 2);
    await navigator.clipboard.writeText(json);
    const orig = exportBtn.textContent;
    exportBtn.textContent = 'Copied!';
    exportBtn.classList.add('btn-flash');
    setTimeout(() => {
      exportBtn.textContent = orig;
      exportBtn.classList.remove('btn-flash');
    }, 1500);
  } catch (e) {
    console.error('export failed:', e);
    alert('Failed to copy settings: ' + e);
  }
});

// ---- Import settings from clipboard ----
importBtn.addEventListener('click', async () => {
  try {
    const text = await navigator.clipboard.readText();
    if (!text.trim()) {
      alert('Clipboard is empty.');
      return;
    }
    let parsed;
    try {
      parsed = JSON.parse(text);
    } catch {
      alert('Clipboard does not contain valid JSON.');
      return;
    }
    // Accept either { sessions: [...] } or the raw sessions array.
    let sessions;
    if (Array.isArray(parsed)) {
      sessions = parsed;
    } else if (parsed.sessions && Array.isArray(parsed.sessions)) {
      sessions = parsed.sessions;
    } else {
      alert('Clipboard JSON must have a "sessions" array.');
      return;
    }
    // Basic shape check: each session needs name + parts.
    for (const s of sessions) {
      if (!s.name || typeof s.name !== 'string') {
        alert('Each session must have a string "name".');
        return;
      }
      if (!Array.isArray(s.parts) || s.parts.length === 0) {
        alert('Session "' + s.name + '" must have a non-empty "parts" array.');
        return;
      }
      for (const p of s.parts) {
        if (typeof p.minutes !== 'number' || p.minutes < 1 || p.minutes > 120) {
          alert('Each part in "' + s.name + '" must have "minutes" between 1 and 120.');
          return;
        }
      }
    }

    const newSettings = await invoke('update_settings', { sessions, shortcuts: undefined });
    sessions = newSettings.sessions;
    sessionIds = newSettings.sessions.map(s => s.id);
    if (newSettings.shortcuts) {
      shortcutConfig = newSettings.shortcuts;
    }

    // Refresh the form if the settings panel is still open.
    const edit = overlay._editSessions;
    if (edit) {
      const imported = newSettings.sessions.map(s => ({
        id: s.id,
        name: s.name,
        parts: s.parts.map(p => ({
          name: p.name, minutes: p.minutes,
          extendable: p.extendable || false,
          track_time: p.track_time || false,
        })),
      }));
      overlay._editSessions = imported;
      buildSettingsForm(imported);
    }

    const orig = importBtn.textContent;
    importBtn.textContent = 'Imported!';
    importBtn.classList.add('btn-flash');
    setTimeout(() => {
      importBtn.textContent = orig;
      importBtn.classList.remove('btn-flash');
    }, 1500);
  } catch (e) {
    console.error('import failed:', e);
    alert('Failed to import settings: ' + e);
  }
});

// Close overlay on backdrop click.
overlay.addEventListener('click', (e) => {
  if (e.target === overlay) {
    closeSettings();
  }
});

// ---- Initial load ----
(async () => {
  try {
    const [tick, settings, docked] = await Promise.all([
      invoke('get_state'),
      invoke('get_settings'),
      invoke('get_dock_state'),
    ]);
    sessions = settings.sessions;
    sessionIds = settings.sessions.map(s => s.id);
    if (settings.shortcuts) {
      shortcutConfig = settings.shortcuts;
    }
    render(tick);
    setDocked(docked);
  } catch (e) {
    console.error('init failed:', e);
  }
})();
