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
let wasRunning = false;
let lastPartName = '';
let isDocked = false;

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
const continueBtn = document.getElementById('continue-btn');
const sessionLeftBtn = document.getElementById('session-left');
const sessionRightBtn = document.getElementById('session-right');
const settingsBtn = document.getElementById('settings-btn');
const overlay = document.getElementById('settings-overlay');
const sessionsContainer = document.getElementById('sessions-container');
const addSessionBtn = document.getElementById('add-session-btn');
const saveBtn = document.getElementById('save-settings');
const cancelBtn = document.getElementById('cancel-settings');
const dailyTotalEl = document.getElementById('daily-total');

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

function render(tick) {
  // Beep on timer-driven transitions only (not manual switches or startup).
  const partChanged = tick.partName !== lastPartName;
  if (tick.running && !wasRunning) {
    // Session started (user clicked Start) — single long beep.
    beep(660, 600, 1);
  } else if (partChanged && tick.running && !tick.paused && !isPaused) {
    // Timer auto-advanced to the next part — short triple beep.
    beep(880, 150, 3);
  } else if (partChanged && !tick.running && wasRunning) {
    // Session finished (last part ended, timer stopped) — single long beep.
    beep(660, 600, 1);
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

  // Show/hide Continue button based on paused state.
  if (tick.paused) {
    continueBtn.classList.remove('hidden');
  } else {
    continueBtn.classList.add('hidden');
  }

  if (tick.running) {
    toggleBtn.textContent = 'Stop';
    toggleBtn.classList.add('is-running');
  } else {
    toggleBtn.textContent = 'Start';
    toggleBtn.classList.remove('is-running');
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

// ---- Continue button ----
continueBtn.addEventListener('click', async () => {
  try {
    await invoke('continue_timer');
  } catch (e) {
    console.error('continue_timer failed:', e);
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

  // Space/Enter to continue when paused (overtime) — not in dock mode.
  if (!isDocked && isPaused && (e.key === ' ' || e.key === 'Enter')) {
    e.preventDefault();
    try { await invoke('continue_timer'); } catch (_) {}
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

  // Store reference for save.
  overlay._editSessions = editSessions;

  overlay.classList.remove('hidden');
  // Push dummy history entry so Android back button closes overlay
  // instead of exiting the app.
  window.history.pushState({ overlayOpen: true }, '');
});

// Android back button: close settings overlay instead of exiting app
window.addEventListener('popstate', (e) => {
  if (!overlay.classList.contains('hidden')) {
    overlay.classList.add('hidden');
    e.preventDefault();
  }
});

cancelBtn.addEventListener('click', () => {
  overlay.classList.add('hidden');
  // Remove the dummy history entry when closing via Cancel.
  if (window.history.state && window.history.state.overlayOpen) {
    window.history.back();
  }
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
    const newSettings = await invoke('update_settings', { sessions: edit });
    sessions = newSettings.sessions;
    sessionIds = newSettings.sessions.map(s => s.id);
    overlay.classList.add('hidden');
  } catch (e) {
    console.error('update_settings failed:', e);
    alert('Failed to save: ' + e);
  }
});

// Close overlay on backdrop click.
overlay.addEventListener('click', (e) => {
  if (e.target === overlay) {
    overlay.classList.add('hidden');
  }
});

// ---- Initial load ----
(async () => {
  try {
    const [tick, daily, settings, docked] = await Promise.all([
      invoke('get_state'),
      invoke('get_daily_total'),
      invoke('get_settings'),
      invoke('get_dock_state'),
    ]);
    sessions = settings.sessions;
    sessionIds = settings.sessions.map(s => s.id);
    render(tick);
    dailyTotalEl.textContent = 'Today: ' + formatDailyTotal(daily);
    setDocked(docked);
  } catch (e) {
    console.error('init failed:', e);
  }
})();
