// The window is a thin client: every action is one Tauri command. State lives in the
// queue file, not here, so reloading or restarting the app loses nothing.
const { invoke } = window.__TAURI__.core;

const statusEl = document.getElementById("status");
const resultsEl = document.getElementById("results");
const emptyEl = document.getElementById("empty");
const errorEl = document.getElementById("error");

function showError(error) {
  errorEl.textContent = String(error);
  errorEl.hidden = false;
}

function clearError() {
  errorEl.hidden = true;
}

async function refreshStatus() {
  const { waiting, in_progress: inProgress } = await invoke("status");
  statusEl.textContent = `${waiting} waiting, ${inProgress} in progress`;
}

function addResult({ task_id: taskId, text }) {
  const item = document.createElement("li");
  const id = document.createElement("span");
  id.className = "id";
  id.textContent = `task ${taskId}`;
  item.append(id, document.createTextNode(text));
  resultsEl.append(item);
  emptyEl.hidden = true;
}

async function run(action) {
  clearError();
  try {
    await action();
    await refreshStatus();
  } catch (error) {
    showError(error);
  }
}

document.getElementById("submit-form").addEventListener("submit", (event) => {
  event.preventDefault();
  run(() => invoke("submit", { text: document.getElementById("text").value }));
});

document.getElementById("process").addEventListener("click", () =>
  run(async () => {
    const processed = await invoke("process_next");
    if (processed === null) {
      statusEl.textContent = "Nothing waiting.";
    }
  }),
);

document.getElementById("collect").addEventListener("click", () =>
  run(async () => {
    (await invoke("collect")).forEach(addResult);
  }),
);

run(async () => {});
