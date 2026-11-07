const CACHE_NAME = 'deft-reversi-models-v1';

async function digest(blob) {
    const hash = await crypto.subtle.digest('SHA-256', await blob.arrayBuffer());
    return Array.from(new Uint8Array(hash), b => b.toString(16).padStart(2, '0')).join('');
}

/** Load a verified cached model or explicitly ask before downloading one. */
export async function loadModel(model, report = () => {}, requestConsent = async () => false) {
    if (model !== 'classic' && model !== 'pen') throw new Error('不明な評価モデルです');
    report({ stage: 'manifest' });
    let cache = null, blob = null, storageError = false;
    try {
        if (!globalThis.caches) throw new Error('storage unavailable');
        cache = await caches.open(CACHE_NAME);
    } catch { storageError = true; }
    const manifestUrl = new URL('../assets/model-manifest.json', import.meta.url);
    let manifestResponse;
    try {
        manifestResponse = await fetch(manifestUrl, { cache: 'no-cache' });
        if (!manifestResponse.ok) throw new Error(`モデル情報の取得に失敗しました (${manifestResponse.status})`);
        if (cache) {
            try { await cache.put(manifestUrl.href, manifestResponse.clone()); }
            catch { storageError = true; }
        }
    } catch (error) {
        manifestResponse = await cache?.match(manifestUrl.href);
        if (!manifestResponse) throw error;
    }
    const manifest = await manifestResponse.json();
    const info = manifest[model];
    if (!info || info.file !== `eval-${model}.dpr2.gz` ||
        !Number.isSafeInteger(info.compressedBytes) || info.compressedBytes <= 0 ||
        !Number.isSafeInteger(info.uncompressedBytes) || info.uncompressedBytes <= 0 ||
        !/^[a-f0-9]{64}$/.test(info.sha256)) throw new Error('モデル情報が不正です');
    const url = new URL(`../assets/${info.file}`, import.meta.url);
    const cacheKey = new URL(url);
    cacheKey.searchParams.set('sha256', info.sha256);
    try {
        const saved = await cache?.match(cacheKey.href);
        if (saved) {
            const candidate = await saved.blob();
            if (candidate.size === info.compressedBytes && await digest(candidate) === info.sha256) blob = candidate;
            else await cache.delete(cacheKey.href);
        }
    } catch { cache = null; storageError = true; }
    if (!blob) {
        if (!await requestConsent({ model, bytes: info.compressedBytes, storageAvailable: !storageError })) {
            const error = new Error('ダウンロードをキャンセルしました');
            error.name = 'ModelDownloadCancelled';
            throw error;
        }
        const response = await fetch(url);
        if (!response.ok || !response.body) throw new Error(`評価モデルの取得に失敗しました (${response.status})`);
        const chunks = [], reader = response.body.getReader();
        let received = 0, lastReport = 0;
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
        blob = new Blob(chunks);
        report({ stage: 'verify' });
        if (await digest(blob) !== info.sha256) throw new Error('評価モデルの検証に失敗しました');
        if (cache) {
            try {
                await cache.put(cacheKey.href, new Response(blob));
                // Remove older versions of this model only after the new one
                // is verified and safely stored.
                for (const key of await cache.keys()) {
                    if (key.url.startsWith(`${url.href}?sha256=`) && key.url !== cacheKey.href) await cache.delete(key);
                }
            } catch { storageError = true; }
        }
        if (storageError) report({ stage: 'storage-warning' });
    }
    if (!globalThis.DecompressionStream) throw new Error('このブラウザはgzipの展開に対応していません');
    report({ stage: 'decompress', loaded: 0, total: info.uncompressedBytes });
    const unpacker = blob.stream().pipeThrough(new DecompressionStream('gzip')).getReader();
    const bytes = new Uint8Array(info.uncompressedBytes);
    let unpacked = 0, lastReport = 0;
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

export function confirmModelDownload(info) {
    const name = info.model === 'pen' ? 'PEN' : '標準';
    const size = (info.bytes / (1024 * 1024)).toFixed(1);
    const warning = info.storageAvailable ? '' : '\n端末に保存できないため、次回も確認が必要です。';
    return globalThis.confirm(`${name}モデル（約${size} MiB）をダウンロードしますか？${warning}`);
}

export function progressText(progress) {
    if (progress.stage === 'manifest') return 'モデル情報を確認中…';
    if (progress.stage === 'verify') return 'モデルを検証中…';
    if (progress.stage === 'storage-warning') return '端末に保存できませんでした。次回も確認が必要です。';
    if (progress.stage === 'initialize') return 'エンジンを準備中…';
    const label = progress.stage === 'download' ? 'モデルをダウンロード中' : 'モデルを展開中';
    const percent = Math.min(100, Math.floor(progress.loaded * 100 / progress.total));
    const mib = value => (value / (1024 * 1024)).toFixed(1);
    return `${label} ${percent}%（${mib(progress.loaded)} / ${mib(progress.total)} MiB）`;
}
