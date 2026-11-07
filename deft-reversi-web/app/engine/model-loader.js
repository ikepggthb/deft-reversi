/** Download and unpack a model while reporting measured byte counts. */
export async function loadModel(model, report = () => {}) {
    if (model !== 'classic' && model !== 'pen') throw new Error('不明な評価モデルです');
    report({ stage: 'manifest' });
    const manifestResponse = await fetch(new URL('../assets/model-manifest.json', import.meta.url));
    if (!manifestResponse.ok) throw new Error(`モデル情報の取得に失敗しました (${manifestResponse.status})`);
    const manifest = await manifestResponse.json();
    const info = manifest[model];
    if (!info || info.file !== `eval-${model}.dpr2.gz` ||
        !Number.isSafeInteger(info.compressedBytes) || info.compressedBytes <= 0 ||
        !Number.isSafeInteger(info.uncompressedBytes) || info.uncompressedBytes <= 0) {
        throw new Error('モデル情報が不正です');
    }
    const response = await fetch(new URL(`../assets/${info.file}`, import.meta.url));
    if (!response.ok || !response.body) throw new Error(`評価モデルの取得に失敗しました (${response.status})`);
    if (!globalThis.DecompressionStream) throw new Error('このブラウザはgzipの展開に対応していません');

    // Keep each stage separate: the manifest gives a reliable total even if
    // the server or browser does not expose Content-Length (e.g. cached data).
    const chunks = [];
    const reader = response.body.getReader();
    let received = 0;
    let lastReport = 0;
    report({ stage: 'download', loaded: 0, total: info.compressedBytes });
    while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        received += value.byteLength;
        if (received > info.compressedBytes) throw new Error('評価モデルのサイズが一致しません');
        chunks.push(value);
        const now = performance.now();
        if (now - lastReport >= 100 || received === info.compressedBytes) {
            report({ stage: 'download', loaded: received, total: info.compressedBytes });
            lastReport = now;
        }
    }
    if (received !== info.compressedBytes) throw new Error('評価モデルのダウンロードが途中で終了しました');
    report({ stage: 'decompress', loaded: 0, total: info.uncompressedBytes });
    const stream = new Blob(chunks).stream().pipeThrough(new DecompressionStream('gzip'));
    chunks.length = 0;
    const unpacker = stream.getReader();
    const bytes = new Uint8Array(info.uncompressedBytes);
    let unpacked = 0;
    lastReport = 0;
    while (true) {
        const { done, value } = await unpacker.read();
        if (done) break;
        if (unpacked + value.byteLength > bytes.length) throw new Error('展開後のモデルサイズが一致しません');
        bytes.set(value, unpacked);
        unpacked += value.byteLength;
        const now = performance.now();
        if (now - lastReport >= 100 || unpacked === bytes.length) {
            report({ stage: 'decompress', loaded: unpacked, total: bytes.length });
            lastReport = now;
        }
    }
    if (unpacked !== bytes.length) throw new Error('展開後のモデルサイズが一致しません');
    report({ stage: 'initialize' });
    return bytes;
}

export function progressText(progress) {
    if (progress.stage === 'manifest') return 'モデル情報を確認中…';
    if (progress.stage === 'initialize') return 'エンジンを準備中…';
    const label = progress.stage === 'download' ? 'モデルをダウンロード中' : 'モデルを展開中';
    const percent = Math.min(100, Math.floor(progress.loaded * 100 / progress.total));
    const mib = value => (value / (1024 * 1024)).toFixed(1);
    return `${label} ${percent}%（${mib(progress.loaded)} / ${mib(progress.total)} MiB）`;
}
