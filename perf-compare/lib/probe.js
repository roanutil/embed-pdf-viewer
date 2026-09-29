(() => {
  const state = { firstPaintAt: null, frames: [], longTasks: [], changes: [] };
  let last = null;
  const tick = (t) => {
    if (last !== null) state.frames.push(t - last);
    last = t;
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);

  new PerformanceObserver((list) => {
    for (const e of list.getEntries()) state.longTasks.push({ start: e.startTime, duration: e.duration });
  }).observe({ type: 'longtask', buffered: true });

  const root = () => document.querySelector('embedpdf-viewer')?.shadowRoot ?? null;
  const isPageImage = (n) => n instanceof HTMLImageElement && n.getAttribute('draggable') === 'false';
  const visibleImages = () =>
    [...(root()?.querySelectorAll('img[draggable="false"]') ?? [])].filter((img) => {
      const r = img.getBoundingClientRect();
      return r.width > 0 && r.height > 0 && r.bottom > 0 && r.top < innerHeight && r.right > 0 && r.left < innerWidth;
    });
  const allDecoded = () => {
    const imgs = visibleImages();
    return imgs.length > 0 && imgs.every((img) => img.complete && img.naturalWidth > 0);
  };

  // A change is a page image being added, re-sourced, or finishing decode.
  // Recording the decode, not only the src swap, is what makes a slow decode
  // show up in the settled time instead of hiding behind the mutation.
  const mark = () => state.changes.push(performance.now());
  const attach = () => {
    const r = root();
    if (!r) return requestAnimationFrame(attach);
    new MutationObserver((records) => {
      for (const rec of records) {
        if (rec.type === 'attributes' ? isPageImage(rec.target) : [...rec.addedNodes].some(
          (n) => isPageImage(n) || (n instanceof Element && n.querySelector('img[draggable="false"]')),
        )) return mark();
      }
    }).observe(r, { subtree: true, childList: true, attributes: true, attributeFilter: ['src'] });
    // `load` doesn't bubble, so listen in the capture phase.
    r.addEventListener('load', (e) => isPageImage(e.target) && e.target.decode().then(mark, mark), true);
    const firstPaint = () => {
      if (allDecoded()) requestAnimationFrame((t) => (state.firstPaintAt ??= t));
      else requestAnimationFrame(firstPaint);
    };
    firstPaint();
  };
  attach();

  const QUIET_MS = 250;
  window.__probe = {
    get firstPaintAt() {
      return state.firstPaintAt;
    },
    // `sinceMs` is when the action started. `doneAt` is when the flow saw the
    // action's own completion condition (see Task 7), and defaults to
    // `sinceMs`. With `requireChange`, at least one page-image change must
    // happen after `sinceMs`, so an action whose render hasn't started yet
    // can't settle on the quiet left over from the previous one.
    //
    // Resolves with the later of `doneAt` and the last change after `sinceMs`,
    // once QUIET_MS has passed since then with every visible image decoded.
    // That is the time of the last change, not the time it was detected.
    settledAt: (sinceMs, { doneAt = sinceMs, requireChange = true } = {}) =>
      new Promise((resolve, reject) => {
        const deadline = performance.now() + 30_000;
        const check = () => {
          const now = performance.now();
          const after = state.changes.filter((t) => t >= sinceMs);
          const lastChange = after.length ? after[after.length - 1] : null;
          if (now > deadline) {
            const why = requireChange && lastChange === null ? 'action produced no page-image change' : 'page never settled';
            return reject(new Error(why));
          }
          const settledAt = Math.max(lastChange ?? sinceMs, doneAt);
          const ready = (!requireChange || lastChange !== null) && now - settledAt >= QUIET_MS && allDecoded();
          if (ready) return resolve(settledAt);
          setTimeout(check, 16);
        };
        check();
      }),
    frames: () => state.frames.slice(),
    longTasks: () => state.longTasks.slice(),
    reset: () => {
      state.frames = [];
      state.longTasks = [];
      state.changes = [];
      last = null;
    },
  };
})();
