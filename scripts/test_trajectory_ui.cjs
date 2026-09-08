// Browser regression for the embedded Trajectory page. Requires Playwright.
// NODE_PATH=/path/to/node_modules CHROME_BIN=/path/to/chrome node scripts/test_trajectory_ui.cjs
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require('playwright');
const html = fs.readFileSync(path.join(__dirname, '../crates/codegen/shell/src/session/trajectory.html'), 'utf8');
const origin = 'http://trajectory.test';
const outputDir = path.join(require('node:os').tmpdir(), 'grow-trajectory-qa');
fs.mkdirSync(outputDir, { recursive: true });

function fixture(count = 360) {
  return Array.from({ length: count }, (_, index) => {
    const turn = Math.floor(index / 30), offset = index % 30;
    const actor = turn % 3 === 1 ? 'subagent:child' : 'main';
    const source = actor === 'main' ? 'root' : 'child';
    const kind = offset === 0 ? 'turn.started' : offset % 6 === 1 ? 'step.started' : offset % 6 === 5 ? 'step.ended' : offset % 2 ? 'tool.result' : 'message.assistant';
    return {
      entry_id: `t:${source}/${index}`, source, seq: index, nesting_path: [index], at_ms: 1788753600000 + index * 1000,
      actor, turn_id: `00000000-0000-7000-8000-${String(turn).padStart(12, '0')}`,
      ...(offset ? { step_index: Math.floor((offset - 1) / 6) } : {}),
      kind, class: kind.startsWith('turn.') || kind.startsWith('step.') ? 'lifecycle' : 'message',
      layer: kind === 'tool.result' ? 'tool.result' : kind === 'message.assistant' ? 'assistant' : 'meta',
      producer: kind === 'tool.result' ? 'tool:read_file' : 'model', state: 'completed', visibility: 'current',
      summary: kind === 'tool.result' ? 'Read source files and inspect implementation' : 'Review the implementation and validate the next change',
      duration_ms: 1234, correlation_id: `call_${offset}`, repair_source_count: 0,
    };
  });
}

(async () => {
  const browser = await chromium.launch({ ...(process.env.CHROME_BIN ? { executablePath: process.env.CHROME_BIN } : {}) });
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  const errors = [], requests = [];
  let records = fixture();
  page.on('pageerror', error => errors.push(error.message));
  await page.route(origin + '/**', async route => {
    const url = new URL(route.request().url()), q = url.searchParams;
    if (url.pathname === '/') return route.fulfill({ contentType: 'text/html', body: html });
    requests.push(url);
    if (url.pathname.endsWith('/event')) return route.fulfill({ json: { row: records.find(row => row.entry_id === q.get('entry')), detailsTruncated: false } });
    let matching = records.filter(row => {
      for (const key of ['actor', 'layer', 'producer']) if (q.get(key) && row[key] !== q.get(key) && !row[key].startsWith(q.get(key) + ':') && !row[key].startsWith(q.get(key) + '.')) return false;
      if (q.get('source') && row.source !== q.get('source')) return false;
      if (q.get('class') && row.class !== q.get('class')) return false;
      if (q.get('turn') && row.turn_id !== q.get('turn')) return false;
      if (q.has('step') && row.step_index !== Number(q.get('step'))) return false;
      if (q.get('related_to')) {
        const target = records.find(item => item.entry_id === q.get('related_to'));
        const ids = new Set([...(target?.full_relation_ids || target?.relation_ids || []), target?.correlation_id].filter(Boolean));
        if (!target || row.source !== target.source || ![...(row.relation_ids || []), row.correlation_id].some(id => ids.has(id))) return false;
      }
      if (q.get('correlation') && row.correlation_id !== q.get('correlation') && !row.relation_ids?.includes(q.get('correlation'))) return false;
      if (q.get('search') && !JSON.stringify(row).includes(q.get('search'))) return false;
      return true;
    }).map(({ full_relation_ids, ...row }, ordinal) => ({ ...row, ordinal }));
    const limit = Number(q.get('limit') || 240);
    let start = Math.max(0, matching.length - limit), end = matching.length;
    if (q.has('entry')) { start = Math.max(0, matching.findIndex(row => row.entry_id === q.get('entry')) - 100); end = Math.min(matching.length, start + limit); }
    if (q.has('before')) { end = matching.findIndex(row => row.entry_id === q.get('before')); start = Math.max(0, end - limit); }
    if (q.has('after')) { start = matching.findIndex(row => row.entry_id === q.get('after')) + 1; end = Math.min(matching.length, start + limit); }
    const dimension = q.get('overview_by') || 'interaction';
    const lane = row => dimension === 'interaction' ? row.layer.startsWith('tool') ? 'tools' : row.producer === 'model' ? 'model' : 'input' : row[dimension].split(/[.:]/)[0];
    const count = rows => rows.reduce((counts, row) => (counts[lane(row)] = (counts[lane(row)] || 0) + 1, counts), {});
    const bins = [];
    for (let i = 0; i < matching.length; i += 6) {
      const rows = matching.slice(i, i + 6);
      bins.push({ first_entry_id: rows[0].entry_id, last_entry_id: rows.at(-1).entry_id, start_ms: rows[0].at_ms, end_ms: rows.at(-1).at_ms, counts: count(rows), turns: rows.filter(row => row.kind === 'turn.started').length, steps: rows.filter(row => row.kind === 'step.started').length });
    }
    await route.fulfill({ json: {
      sessionId: 'Trajectory interaction fixture', eventCount: records.length, matchingCount: matching.length,
      rows: matching.slice(start, end), hasEarlier: start > 0, hasLater: end < matching.length, activeTurn: records.at(-1)?.turn_id, activeStep: 4,
      overview: { dimension, counts: count(matching), bins, start_ms: matching[0]?.at_ms, end_ms: matching.at(-1)?.at_ms },
    } });
  });
  const ready = () => page.waitForFunction(() => document.querySelector('#health').textContent.includes('Connected'));
  const navigate = async query => { await page.goto(origin + '/' + (query || '')); await ready(); };
  try {
    await navigate();
    await page.locator('[data-category="lifecycle"]').click();
    await page.waitForFunction(() => window.__trajectory.length && window.__trajectory.every(row => row.class === 'lifecycle'));
    assert.equal(await page.locator('[data-category="lifecycle"]').getAttribute('aria-pressed'), 'true');
    await page.locator('[data-category=""]').click();
    await page.waitForFunction(() => window.__trajectory.some(row => row.class === 'message'));
    console.log('PASS category filtering and active state');

    await navigate('?actor=subagent%3Achild&producer=tool%3Aread_file&layer=tool.result');
    assert.equal(await page.locator('#actor').inputValue(), 'subagent:child');
    assert.equal(await page.locator('#producer').inputValue(), 'tool:read_file');
    assert.equal(await page.locator('#layer').inputValue(), 'tool.result');
    assert.match(await page.locator('#activeFilters').innerText(), /subagent:child/);
    assert.equal(requests.at(-1).searchParams.get('producer'), 'tool:read_file');
    console.log('PASS exact classification deep links');

    await navigate();
    const firstTurnEntry = await page.locator('#turnJump option').nth(1).getAttribute('value');
    await page.locator('#turnJump').selectOption(firstTurnEntry);
    assert.equal(await page.locator('#inspector').getAttribute('aria-hidden'), 'true');
    assert.equal(decodeURIComponent(new URL(page.url()).hash.slice(1)), firstTurnEntry);
    assert.equal(await page.locator('#nextStep').isEnabled(), true);
    await page.locator('#nextStep').click();
    assert.equal(await page.locator('#stepJump option:checked').textContent(), 'Step 0');
    await page.locator('#nextStep').click();
    assert.equal(await page.locator('#stepJump option:checked').textContent(), 'Step 1');
    await page.locator('#prevStep').click();
    assert.equal(await page.locator('#stepJump option:checked').textContent(), 'Step 0');
    await page.locator('#onlyStep').click();
    await page.waitForFunction(() => window.__trajectory.length && window.__trajectory.every(row => row.step_index === 0));
    assert.equal(new URL(page.url()).searchParams.get('step'), '0');
    assert.equal(new Set(await page.evaluate(() => window.__trajectory.map(row => row.turn_id))).size, 1);
    console.log('PASS turn navigation, step zero, previous/next and scoped actions');

    await navigate();
    const stepButton = page.locator('.event-row [data-scope="step"]').first();
    await stepButton.focus();
    await page.evaluate(() => renderLedger());
    assert.equal(await page.evaluate(() => document.activeElement.dataset.scope), 'step');
    await page.keyboard.press('Enter');
    await page.waitForURL(url => url.searchParams.has('step'));
    await ready();
    assert.equal(await page.locator('#inspector').getAttribute('aria-hidden'), 'true');
    assert.ok(new URL(page.url()).searchParams.has('step'));
    assert.ok(new URL(page.url()).searchParams.has('source'));
    console.log('PASS keyboard activation of inline scope without opening inspector');

    const memberIds = Array.from({ length: 20 }, (_, i) => 'input-' + i);
    records = memberIds.map((id, i) => ({ ...fixture(1)[0], entry_id: 't:root/' + i, seq: i, correlation_id: id }));
    records.push({ ...records[0], source: 'child', actor: 'subagent:child', entry_id: 't:child/0' });
    records.push({ ...records[0], entry_id: 't:root/20', seq: 20, correlation_id: null, relation_ids: memberIds.slice(0, 16), full_relation_ids: memberIds, relation_count: 20 });
    await navigate();
    await page.locator('[data-entry="t:root/20"] .kind').click();
    await page.locator('#relations button', { hasText: 'Related events' }).click();
    await page.waitForFunction(() => window.__trajectory.length === 21);
    assert.equal(new URL(page.url()).searchParams.get('source'), 'root');
    assert.equal(new URL(page.url()).searchParams.get('related_to'), 't:root/20');
    assert.ok(await page.evaluate(() => window.__trajectory.some(row => row.correlation_id === 'input-19')));
    assert.ok(await page.evaluate(() => window.__trajectory.every(row => row.actor === 'main')));
    await page.reload(); await ready();
    assert.equal(requests.at(-1).searchParams.get('related_to'), 't:root/20');
    await page.locator('#relationScope').click(); await ready();
    assert.equal(new URL(page.url()).searchParams.has('source'), false);
    assert.equal(new URL(page.url()).searchParams.has('related_to'), false);
    console.log('PASS source-scoped batch relations beyond summary IDs and URL round trip');

    records = fixture();
    await navigate();
    await page.evaluate(() => { clearTimeout(timer); pauseFollow(); ledger.scrollTop -= 200; });
    records.push(fixture(361).at(-1));
    await page.evaluate(() => poll());
    assert.equal(await page.evaluate(() => selected), null);
    assert.equal(await page.evaluate(() => hasLater), true);
    await page.evaluate(() => loadLater());
    assert.ok(await page.evaluate(() => window.__trajectory.some(row => row.seq === 360)));
    console.log('PASS paused unselected tail can load arriving events');

    await navigate('?search=nonexistent-event');
    assert.equal(await page.locator('#turnJump').isDisabled(), true);
    assert.equal(await page.locator('#onlyStep').isDisabled(), true);
    assert.match(await page.locator('#rows').innerText(), /No matching events/);
    console.log('PASS empty filtered view');

    await navigate();
    await page.locator('#turnJump').selectOption({ index: 2 });
    await page.screenshot({ animations: 'disabled', path: path.join(outputDir, 'desktop-light.png') });
    await page.locator('#themeToggle').click();
    await page.screenshot({ animations: 'disabled', path: path.join(outputDir, 'desktop-dark.png') });
    for (const width of [1024, 768, 375]) {
      await page.setViewportSize({ width, height: 900 });
      await page.locator('#filterToggle').click();
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'Page overflow at '+width);
      assert.ok(await page.locator('#ledger').evaluate(el => el.clientHeight > 100));
      assert.ok(await page.locator('#turnJump').isVisible());
      await page.screenshot({ animations: 'disabled', path: path.join(outputDir, `width-${width}.png`) });
      await page.locator('#filterToggle').click();
    }
    await page.locator('.event-row .kind').first().click();
    assert.equal(await page.locator('#inspector').getAttribute('aria-modal'), 'true');
    assert.equal(await page.locator('.event-browser').evaluate(el => el.inert), true);
    await page.keyboard.press('Escape');
    assert.equal(await page.locator('.event-browser').evaluate(el => el.inert), false);
    console.log('PASS responsive layout, dark/light rendering and drawer focus boundary');
    assert.deepEqual(errors, []);
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
