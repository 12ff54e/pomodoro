// ---- State (mirrored from Rust, updated by events) ----
let sessions = [];
let activeSessionIndex = 0;
let currentPartName = 'Work';
let currentSessionName = 'Pomodoro';
let sessionCount = 1;
let isRunning = false;
let wasRunning = false;
let lastPartName = '';

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
const toggleBtn = document.getElementById('toggle-btn');
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
  const m = Math.floor(totalSeconds / 60);
  const s = totalSeconds % 60;
  return String(m).padStart(2, '0') + ':' + String(s).padStart(2, '0');
}

function formatDailyTotal(totalSeconds) {
  if (!totalSeconds || totalSeconds === 0) return '0m';
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  if (hours > 0) return hours + 'h ' + minutes + 'm';
  return minutes + 'm';
}

/** Map a part name to a CSS class for the phase badge. */
function phaseClass(partName) {
  const key = partName.toLowerCase();
  if (key === 'work') return 'phase-work';
  if (key === 'break') return 'phase-break';
  if (key === 'play') return 'phase-play';
  return 'phase-default';
}

function render(tick) {
  // Beep on timer-driven transitions only (not manual switches or startup).
  const partChanged = tick.partName !== lastPartName;
  if (partChanged && tick.running) {
    // Timer auto-advanced to the next part — short triple beep.
    beep(880, 150, 3);
  } else if (partChanged && !tick.running && wasRunning) {
    // Session finished (last part ended, timer stopped) — single long beep.
    beep(660, 600, 1);
  }
  if (partChanged) {
    lastPartName = tick.partName;
  }
  wasRunning = tick.running;

  currentPartName = tick.partName;
  currentSessionName = tick.sessionName;
  activeSessionIndex = tick.activeSessionIndex;
  sessionCount = tick.sessionCount;
  isRunning = tick.running;

  timerEl.textContent = formatTime(tick.remainingSeconds);
  phaseEl.textContent = tick.partName.toUpperCase();
  phaseEl.className = phaseClass(tick.partName);
  sessionLabelEl.textContent = tick.sessionName;

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

// ---- Session switcher arrows ----
sessionLeftBtn.addEventListener('click', async () => {
  if (isRunning || sessionCount <= 1) return;
  const prev = (activeSessionIndex - 1 + sessionCount) % sessionCount;
  try {
    await invoke('switch_session', { index: prev });
  } catch (e) {
    console.error('switch_session failed:', e);
  }
});

sessionRightBtn.addEventListener('click', async () => {
  if (isRunning || sessionCount <= 1) return;
  const next = (activeSessionIndex + 1) % sessionCount;
  try {
    await invoke('switch_session', { index: next });
  } catch (e) {
    console.error('switch_session failed:', e);
  }
});

// ---- Keyboard shortcuts ----
document.addEventListener('keydown', async (e) => {
  if (e.target.tagName === 'INPUT') return;
  if (isRunning || sessionCount <= 1) return;

  if (e.key === 'ArrowLeft' || e.key === 'h') {
    const prev = (activeSessionIndex - 1 + sessionCount) % sessionCount;
    try { await invoke('switch_session', { index: prev }); } catch (_) {}
  } else if (e.key === 'ArrowRight' || e.key === 'l') {
    const next = (activeSessionIndex + 1) % sessionCount;
    try { await invoke('switch_session', { index: next }); } catch (_) {}
  }
});

// ---- Dynamic settings form ----

/** Create a new default session (Work → Break). */
function makeDefaultSession() {
  return {
    name: 'Work / Break',
    parts: [
      { name: 'Work', minutes: 25 },
      { name: 'Break', minutes: 5 },
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

      const partName = document.createElement('input');
      partName.type = 'text';
      partName.value = part.name;
      partName.placeholder = 'Part name';
      partName.addEventListener('input', () => {
        editSessions[si].parts[pi].name = partName.value;
      });

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

      row.appendChild(partName);
      row.appendChild(partMin);
      row.appendChild(rmBtn);
      partsList.appendChild(row);
    });

    // Add part button.
    const addPart = document.createElement('button');
    addPart.className = 'btn-add-part';
    addPart.textContent = '+ Add Part';
    addPart.addEventListener('click', () => {
      editSessions[si].parts.push({ name: 'Rest', minutes: 5 });
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
    name: s.name,
    parts: s.parts.map(p => ({ name: p.name, minutes: p.minutes })),
  }));
  buildSettingsForm(editSessions);

  // Store reference for save.
  overlay._editSessions = editSessions;

  overlay.classList.remove('hidden');
});

cancelBtn.addEventListener('click', () => {
  overlay.classList.add('hidden');
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
      if (!p.name.trim()) {
        alert('Each part must have a name.');
        return;
      }
      if (!p.minutes || p.minutes < 1 || p.minutes > 120) {
        alert('Part minutes must be between 1 and 120.');
        return;
      }
    }
  }

  try {
    const newSettings = await invoke('update_settings', { sessions: edit });
    sessions = newSettings.sessions;
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
    const [tick, daily, settings] = await Promise.all([
      invoke('get_state'),
      invoke('get_daily_total'),
      invoke('get_settings'),
    ]);
    sessions = settings.sessions;
    render(tick);
    dailyTotalEl.textContent = 'Today: ' + formatDailyTotal(daily);
  } catch (e) {
    console.error('init failed:', e);
  }
})();
