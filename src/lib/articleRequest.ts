// Route/component mounts share the sequence. A timestamp base also keeps hot
// reloads ahead of the backend's current revision without a growing ID map.
let lastRevision = 0;
export function nextArticleRevision(): number {
  lastRevision = Math.max(lastRevision + 1, Date.now() * 1000);
  return lastRevision;
}
