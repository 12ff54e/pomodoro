// ---- State (mirrored from Rust, updated by events) ----
let currentPhase = 'work';
let isRunning = false;
let lastPhase = 'work';
let settings = { workMinutes: 25, breakMinutes: 5 };

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
const toggleBtn = document.getElementById('toggle-btn');
const settingsBtn = document.getElementById('settings-btn');
const overlay = document.getElementById('settings-overlay');
const workInput = document.getElementById('work-minutes');
const breakInput = document.getElementById('break-minutes');
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
      // Work done — 3 short high beeps.
      beep(880, 150, 3);
    } else {
      // Break done — 1 longer lower beep.
      beep(660, 400, 1);
    }
    lastPhase = tick.phase;
  }

  currentPhase = tick.phase;
  isRunning = tick.running;

  timerEl.textContent = formatTime(tick.remainingSeconds);

  if (tick.phase === 'work') {
    phaseEl.textContent = 'WORK';
    phaseEl.className = 'phase-work';
  } else {
    phaseEl.textContent = 'BREAK';
    phaseEl.className = 'phase-break';
  }

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

// ---- Settings ----
settingsBtn.addEventListener('click', () => {
  workInput.value = settings.workMinutes;
  breakInput.value = settings.breakMinutes;
  overlay.classList.remove('hidden');
});

cancelBtn.addEventListener('click', () => {
  overlay.classList.add('hidden');
});

saveBtn.addEventListener('click', async () => {
  const wm = parseInt(workInput.value, 10);
  const bm = parseInt(breakInput.value, 10);
  if (isNaN(wm) || isNaN(bm)) return;

  try {
    const newSettings = await invoke('update_settings', {
      workMinutes: wm,
      breakMinutes: bm,
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
