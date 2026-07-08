// ---- State (mirrored from Rust, updated by events) ----
let currentPhase = 'work';
let isRunning = false;
let settings = { workMinutes: 25, breakMinutes: 5 };

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

// ---- Helpers ----
function formatTime(totalSeconds) {
  const m = Math.floor(totalSeconds / 60);
  const s = totalSeconds % 60;
  return String(m).padStart(2, '0') + ':' + String(s).padStart(2, '0');
}

function render(tick) {
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
  } catch (e) {
    console.error('get_state failed:', e);
  }
})();
