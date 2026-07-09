// ---- State (mirrored from Rust, updated by events) ----
let currentPhase = 'work';
let currentSession = 'pomodoro';
let isRunning = false;
let lastPhase = 'work';
let settings = { workMinutes: 25, breakMinutes: 5, playMinutes: 25, playBreakMinutes: 5 };

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
    delay += durationMs / 1000 + 0.15; // gap between beeps
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
const workInput = document.getElementById('work-minutes');
const breakInput = document.getElementById('break-minutes');
const playInput = document.getElementById('play-minutes');
const playBreakInput = document.getElementById('play-break-minutes');
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
  if (!totalSeconds || totalSeconds === 0) {
    return '0m';
  }
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  if (hours > 0) {
    return hours + 'h ' + minutes + 'm';
  }
  return minutes + 'm';
}

function render(tick) {
  // Detect phase transitions and play sound.
  if (tick.phase !== lastPhase) {
    if (tick.phase === 'break') {
      // Focus/play session done — 3 short high beeps.
      beep(880, 150, 3);
    } else {
      // Break done — 1 longer lower beep.
      beep(660, 400, 1);
    }
    lastPhase = tick.phase;
  }

  currentPhase = tick.phase;
  currentSession = tick.sessionType || 'pomodoro';
  isRunning = tick.running;

  timerEl.textContent = formatTime(tick.remainingSeconds);

  // Phase badge.
  if (tick.phase === 'work') {
    phaseEl.textContent = 'WORK';
    phaseEl.className = 'phase-work';
  } else if (tick.phase === 'play') {
    phaseEl.textContent = 'PLAY';
    phaseEl.className = 'phase-play';
  } else {
    phaseEl.textContent = 'BREAK';
    phaseEl.className = 'phase-break';
  }

  // Session label.
  sessionLabelEl.textContent = currentSession === 'playbreak' ? 'Play / Break' : 'Pomodoro';

  if (tick.running) {
    toggleBtn.textContent = 'Stop';
    toggleBtn.classList.add('is-running');
  } else {
    toggleBtn.textContent = 'Start';
    toggleBtn.classList.remove('is-running');
  }

  // Update daily total if present in the tick.
  if (tick.dailyTotalSeconds !== undefined) {
    dailyTotalEl.textContent = 'Today: ' + formatDailyTotal(tick.dailyTotalSeconds);
  }
}

// ---- Tauri API (global Tauri enabled) ----
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// ---- Event: timer tick from backend ----
listen('timer-tick', (event) => {
  render(event.payload);
});

// ---- Toggle button ----
toggleBtn.addEventListener('click', async () => {
  if (isRunning) {
    try {
      await invoke('stop_timer');
    } catch (e) {
      console.error('stop_timer failed:', e);
    }
  } else {
    try {
      await invoke('start_timer');
    } catch (e) {
      console.error('start_timer failed:', e);
    }
  }
});

// ---- Session switcher arrows ----
const sessionOrder = ['pomodoro', 'playbreak'];

function nextSession() {
  const idx = sessionOrder.indexOf(currentSession);
  return sessionOrder[(idx + 1) % sessionOrder.length];
}

function prevSession() {
  const idx = sessionOrder.indexOf(currentSession);
  return sessionOrder[(idx - 1 + sessionOrder.length) % sessionOrder.length];
}

sessionLeftBtn.addEventListener('click', async () => {
  if (isRunning) return;
  try {
    await invoke('switch_session', { sessionType: prevSession() });
  } catch (e) {
    console.error('switch_session failed:', e);
  }
});

sessionRightBtn.addEventListener('click', async () => {
  if (isRunning) return;
  try {
    await invoke('switch_session', { sessionType: nextSession() });
  } catch (e) {
    console.error('switch_session failed:', e);
  }
});

// ---- Keyboard shortcuts ----
document.addEventListener('keydown', async (e) => {
  // Ignore when typing in inputs.
  if (e.target.tagName === 'INPUT') return;
  if (isRunning) return;

  if (e.key === 'ArrowLeft' || e.key === 'h') {
    try {
      await invoke('switch_session', { sessionType: prevSession() });
    } catch (err) {
      console.error('switch_session failed:', err);
    }
  } else if (e.key === 'ArrowRight' || e.key === 'l') {
    try {
      await invoke('switch_session', { sessionType: nextSession() });
    } catch (err) {
      console.error('switch_session failed:', err);
    }
  }
});

// ---- Settings ----
settingsBtn.addEventListener('click', () => {
  workInput.value = settings.workMinutes;
  breakInput.value = settings.breakMinutes;
  playInput.value = settings.playMinutes;
  playBreakInput.value = settings.playBreakMinutes;
  overlay.classList.remove('hidden');
});

cancelBtn.addEventListener('click', () => {
  overlay.classList.add('hidden');
});

saveBtn.addEventListener('click', async () => {
  const wm = parseInt(workInput.value, 10);
  const bm = parseInt(breakInput.value, 10);
  const pm = parseInt(playInput.value, 10);
  const pbm = parseInt(playBreakInput.value, 10);
  if (isNaN(wm) || isNaN(bm) || isNaN(pm) || isNaN(pbm)) return;

  try {
    const newSettings = await invoke('update_settings', {
      workMinutes: wm,
      breakMinutes: bm,
      playMinutes: pm,
      playBreakMinutes: pbm,
    });
    settings = newSettings;
    overlay.classList.add('hidden');
  } catch (e) {
    console.error('update_settings failed:', e);
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
    const tick = await invoke('get_state');
    render(tick);
    const daily = await invoke('get_daily_total');
    dailyTotalEl.textContent = 'Today: ' + formatDailyTotal(daily);
  } catch (e) {
    console.error('init failed:', e);
  }
})();
