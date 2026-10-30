import { compareAnswer, formatDuration, nodesPerSecond } from './model.js';

const el = id => document.getElementById(id);
const ui = Object.fromEntries(['first','last','goal','goal-value','value-field','value-label','limit','tt','start','stop','status','download','count','metric-count','metric-time','metric-nodes','metric-nps','progress','rows'].map(id => [id, el(id)]));
let worker = null, selected = [], results = [], cursor = 0, request, running = false;
let currentWorkerId = 0;

function setStatus(message, error = false) { ui.status.textContent = message; ui.status.classList.toggle('error', error); }
function setRunning(value) {
    running = value;
    for (const id of ['first','last','goal','goal-value','limit','tt','start']) ui[id].disabled = value;
    ui.stop.disabled = !value;
    ui.download.disabled = !results.length;
}
function createCell(row, text, className = '') {
    const cell = document.createElement('td');
    cell.textContent = text;
    if (className) cell.className = className;
    row.appendChild(cell);
}
function summary() {
    const time = results.reduce((sum, item) => sum + item.result.elapsedMs, 0);
    const nodes = results.reduce((sum, item) => sum + BigInt(item.result.nodes), 0n);
    ui['metric-count'].textContent = `${results.length} / ${selected.length}`;
    ui['metric-time'].textContent = formatDuration(time);
    ui['metric-nodes'].textContent = nodes.toLocaleString('en-US');
    ui['metric-nps'].textContent = nodesPerSecond(nodes, time);
    const pct = selected.length ? Math.round(results.length * 100 / selected.length) : 0;
    ui.progress.style.width = `${pct}%`;
    ui.progress.parentElement.setAttribute('aria-valuenow', String(pct));
    ui.count.textContent = `${results.length} / ${selected.length} 局面を測定`;
    ui.download.disabled = !results.length;
}
function renderResult(item, result) {
    const row = document.createElement('tr');
    const check = compareAnswer(result, item.answers);
    const state = result.solved ? '完全読了' : result.stopReason === 'TimeLimit' ? '時間上限' : result.stopReason === 'NodeLimit' ? 'ノード上限' : result.stopReason === 'Completed' ? '深さ到達' : result.stopReason;
    createCell(row, `FFO ${item.number}`);
    createCell(row, state);
    createCell(row, result.bestMove ?? '—');
    createCell(row, result.score == null ? '—' : `${result.score > 0 ? '+' : ''}${result.score}`);
    createCell(row, formatDuration(result.elapsedMs));
    createCell(row, BigInt(result.nodes).toLocaleString('en-US'));
    createCell(row, nodesPerSecond(result.nodes, result.elapsedMs));
    createCell(row, check === null ? '—' : check ? '一致' : '不一致', check === null ? '' : check ? 'good' : 'bad');
    ui.rows.appendChild(row);
    return state;
}
function finish(message, error = false) {
    if (worker) worker.terminate();
    worker = null;
    currentWorkerId++;
    setRunning(false);
    setStatus(message, error);
}
function next() {
    if (cursor >= selected.length) { finish('ベンチマークが完了しました'); return; }
    const item = selected[cursor];
    setStatus(`FFO ${item.number} を探索中…（${cursor + 1} / ${selected.length}）`);
    worker.postMessage({ type: 'solve', number: item.number, position: item.position, request });
}
function integer(input, min, max, label) {
    const value = input instanceof HTMLSelectElement ? Number(input.value) : input.valueAsNumber;
    if (!Number.isInteger(value) || value < min || value > max) throw new Error(`${label}は${min}〜${max}の整数で指定してください`);
    return value;
}
ui.goal.addEventListener('change', () => {
    ui['value-field'].hidden = ui.goal.value === 'endgame';
    ui['value-label'].textContent = ui.goal.value === 'level' ? '探索レベル' : '探索の深さ';
});
ui.start.addEventListener('click', async () => {
    if (running) return;
    let workerId;
    try {
        const first = integer(ui.first, 1, 79, '開始番号');
        const last = integer(ui.last, 1, 79, '終了番号');
        if (first > last) throw new Error('終了番号は開始番号以上にしてください');
        const limit = integer(ui.limit, 0, 4294967, '時間上限');
        const ttMiB = integer(ui.tt, 1, 256, '置換表');
        const goal = ui.goal.value === 'endgame' ? { kind: 'endgame' } : { kind: ui.goal.value, value: integer(ui['goal-value'], 1, 60, '深さ・レベル') };
        request = { goal, ...(limit ? { timeLimitMs: limit * 1000 } : {}) };
        results = []; cursor = 0; selected = []; ui.rows.replaceChildren();
        for (const id of ['metric-count','metric-time','metric-nodes','metric-nps']) ui[id].textContent = '—';
        ui.count.textContent = '局面を読み込んでいます';
        ui.progress.style.width = '0%';
        ui.progress.parentElement.setAttribute('aria-valuenow', '0');
        setRunning(true);
        setStatus('局面とエンジンを読み込んでいます…');
        workerId = ++currentWorkerId;
        const response = await fetch(new URL('./positions.json', import.meta.url));
        if (!response.ok) throw new Error('FFO局面を読み込めませんでした');
        const positions = await response.json();
        if (workerId !== currentWorkerId) return;
        selected = positions.filter(item => item.number >= first && item.number <= last);
        if (selected.length !== last - first + 1) throw new Error('FFO局面のデータが不足しています');
        summary();
        worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
        worker.onmessage = ({ data }) => {
            if (workerId !== currentWorkerId) return;
            if (data.type === 'error') { finish(data.message, true); return; }
            if (data.type === 'ready') { next(); return; }
            if (data.type === 'result') {
                const item = selected[cursor];
                if (!item || item.number !== data.number) { finish('結果の順序が不正です', true); return; }
                results.push({ item, result: data.result });
                renderResult(item, data.result); cursor++; summary(); next();
            }
        };
        worker.onerror = event => { if (workerId === currentWorkerId) finish(`エンジンの起動に失敗しました: ${event.message || '不明なエラー'}`, true); };
        worker.postMessage({ type: 'init', ttMiB });
    } catch (error) { if (workerId === undefined || workerId === currentWorkerId) finish(error?.message ?? String(error), true); }
});
ui.stop.addEventListener('click', () => finish('停止しました。完了した局面の結果を表示しています。'));
ui.download.addEventListener('click', () => {
    const lines = [['FFO','状態','最善手','評価値','探索時間ms','ノード数','ノード毎秒','正解照合']];
    for (const { item, result } of results) {
        const check = compareAnswer(result, item.answers);
        lines.push([item.number, result.solved ? '完全読了' : result.stopReason, result.bestMove ?? '', result.score ?? '', result.elapsedMs, result.nodes, nodesPerSecond(result.nodes, result.elapsedMs).replaceAll(',', ''), check === null ? '' : check ? '一致' : '不一致']);
    }
    const csv = '\ufeff' + lines.map(line => line.map(value => `"${String(value).replaceAll('"', '""')}"`).join(',')).join('\r\n');
    const url = URL.createObjectURL(new Blob([csv], { type: 'text/csv;charset=utf-8' }));
    const link = document.createElement('a'); link.href = url; link.download = 'deft-ffo-benchmark.csv'; link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
});
