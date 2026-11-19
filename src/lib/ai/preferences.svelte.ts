const KEY = 'aequo:aiAutoOpen';
function read() {
  try {
    return localStorage.getItem(KEY) !== 'false';
  } catch {
    return true;
  }
}
const state = $state({ autoOpen: read() });
export function aiAutoOpen() {
  return state.autoOpen;
}
export function setAiAutoOpen(value: boolean) {
  localStorage.setItem(KEY, String(value));
  state.autoOpen = value;
}
